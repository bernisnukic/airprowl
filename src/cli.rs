use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::ble::BleCompanyDb;
use crate::config::{CliMode, Config, ScanTarget};
use crate::events::AppEvent;
use crate::export;
use crate::names::NameStore;
use crate::oui::OuiDb;
use crate::scanner;
use crate::store::{self, BtDevice, SubGhzDevice, WifiDevice};

/// Run a headless scan for `cli.duration_secs`, then emit JSON to stdout.
/// Stays inside the tokio runtime — the caller's `#[tokio::main]` keeps it alive.
pub async fn run(config: Arc<Config>) -> anyhow::Result<()> {
    let cli = config.cli.clone().expect("cli mode requested but no CliMode set");
    let (tx, mut rx) = mpsc::channel::<AppEvent>(1024);

    spawn_scanners(config.clone(), tx.clone(), &cli);

    let mut bt_devices: HashMap<String, BtDevice> = HashMap::new();
    let mut wifi_devices: HashMap<String, WifiDevice> = HashMap::new();
    let mut subghz_devices: HashMap<String, SubGhzDevice> = HashMap::new();
    let mut status: Vec<String> = Vec::new();

    let deadline = Instant::now() + Duration::from_secs(cli.duration_secs);
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => match event {
                AppEvent::BtUpdate(u) => store::apply_bt_update(&mut bt_devices, u),
                AppEvent::WifiApUpdate(u) => store::apply_wifi_ap_update(&mut wifi_devices, u),
                AppEvent::WifiClientUpdate(u) => store::apply_wifi_client_update(&mut wifi_devices, u),
                AppEvent::SubGhzSignal(s) => store::apply_subghz_update(&mut subghz_devices, s),
                AppEvent::BtError(msg) => status.push(format!("BT error: {}", msg)),
                AppEvent::WifiStatus(Some(msg)) => status.push(msg),
                AppEvent::SubGhzStatus(msg) => status.push(msg),
                AppEvent::WifiStatus(None) | AppEvent::Tick | AppEvent::Key(_) => {}
            },
            Ok(None) => break,
            Err(_) => break, // timeout reached
        }
    }

    drop(tx);

    let oui_db = OuiDb::load();
    let ble_db = BleCompanyDb::load();
    let names = NameStore::load(&config.names_path);
    let names_lookup = |id: &str| names.get(id).map(|s| s.to_string());
    let now = Instant::now();

    let json = match cli.scan {
        ScanTarget::Bt => {
            let list = export::bt_to_export(
                &bt_devices,
                oui_db.as_ref(),
                ble_db.as_ref(),
                &names_lookup,
                now,
            );
            serde_json::json!({
                "scan_type": "bt",
                "duration_secs": cli.duration_secs,
                "device_count": list.len(),
                "status": status,
                "devices": list,
            })
        }
        ScanTarget::Wifi => {
            let list = export::wifi_to_export(
                &wifi_devices,
                oui_db.as_ref(),
                &names_lookup,
                now,
            );
            serde_json::json!({
                "scan_type": "wifi",
                "duration_secs": cli.duration_secs,
                "device_count": list.len(),
                "status": status,
                "devices": list,
            })
        }
        ScanTarget::SubGhz => {
            let list = export::subghz_to_export(&subghz_devices, &names_lookup, now);
            serde_json::json!({
                "scan_type": "subghz",
                "duration_secs": cli.duration_secs,
                "device_count": list.len(),
                "status": status,
                "devices": list,
            })
        }
    };

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn spawn_scanners(
    config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
    cli: &CliMode,
) {
    match cli.scan {
        ScanTarget::Bt => {
            let tx = tx.clone();
            let cfg = config.clone();
            tokio::spawn(async move {
                if let Err(e) = scanner::bluetooth::run_bt_scanner(cfg, tx.clone()).await {
                    tx.send(AppEvent::BtError(format!("{}", e))).await.ok();
                }
            });
        }
        ScanTarget::Wifi => {
            // AP scanner (no root needed)
            let tx_ap = tx.clone();
            let cfg = config.clone();
            tokio::spawn(async move {
                if let Err(e) = scanner::wifi::ap_scanner::run_ap_scanner(cfg, tx_ap.clone()).await {
                    tx_ap.send(AppEvent::WifiStatus(Some(format!("AP scanner error: {}", e))))
                        .await
                        .ok();
                }
            });
            // Best-effort monitor mode for client sniffing (requires root)
            let tx_mon = tx.clone();
            let cfg = config.clone();
            tokio::spawn(async move {
                match scanner::wifi::monitor::setup_monitor(cfg.clone()).await {
                    Err(e) => {
                        tx_mon.send(AppEvent::WifiStatus(Some(format!(
                            "No monitor mode — {}. AP scanning only.",
                            e
                        )))).await.ok();
                    }
                    Ok(mon_handle) => {
                        let known_aps = Arc::new(Mutex::new(HashSet::new()));
                        let cancel = CancellationToken::new();
                        let sniff_iface = mon_handle.sniff_iface.clone();
                        if let Err(e) = scanner::wifi::client_sniffer::run_client_sniffer(
                            sniff_iface,
                            tx_mon.clone(),
                            known_aps,
                            cancel.clone(),
                        ).await {
                            tx_mon.send(AppEvent::WifiStatus(Some(format!("Sniffer error: {}", e))))
                                .await.ok();
                        }
                        cancel.cancel();
                        drop(mon_handle);
                    }
                }
            });
        }
        ScanTarget::SubGhz => {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = scanner::subghz::yardstick::run_subghz_scanner(tx.clone()).await {
                    tx.send(AppEvent::SubGhzStatus(format!("Sub-GHz error: {}", e)))
                        .await
                        .ok();
                }
            });
        }
    }
}
