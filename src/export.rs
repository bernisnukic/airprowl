use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;

use crate::ble::BleCompanyDb;
use crate::oui::{self, BtIdentity, OuiDb};
use crate::signal::{bt_distance, wifi_ap_distance, wifi_client_distance};
use crate::store::{BtDevice, SubGhzDevice, WifiDevice, WifiKind};

#[derive(Serialize)]
pub struct BtExport {
    pub address: String,
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub icon: Option<String>,
    pub company_id: Option<u16>,
    pub is_random: bool,
    pub rssi_dbm: Option<i16>,
    pub rssi_avg_dbm: Option<f64>,
    pub tx_power: Option<i16>,
    pub distance_m: Option<f64>,
    pub samples: u32,
    pub seen_secs_ago: u64,
    pub tracked_secs: u64,
    pub custom_name: Option<String>,
}

#[derive(Serialize)]
pub struct WifiExport {
    pub mac: String,
    pub kind: &'static str,
    pub ssid: Option<String>,
    pub vendor: Option<String>,
    pub probing_for: Option<String>,
    pub signal_pct: Option<f64>,
    pub signal_pct_avg: Option<f64>,
    pub rssi_dbm: Option<i16>,
    pub rssi_avg_dbm: Option<f64>,
    pub channel: Option<String>,
    pub frequency: Option<String>,
    pub security: Option<String>,
    pub distance_m: Option<f64>,
    pub samples: u32,
    pub seen_secs_ago: u64,
    pub tracked_secs: u64,
    pub custom_name: Option<String>,
}

#[derive(Serialize)]
pub struct SubGhzExport {
    pub freq_hz: u32,
    pub freq_mhz: f64,
    pub band: String,
    pub rssi_dbm: i16,
    pub rssi_avg_dbm: Option<f64>,
    pub samples: u32,
    pub seen_secs_ago: u64,
    pub tracked_secs: u64,
    pub custom_name: Option<String>,
}

pub fn bt_to_export(
    devices: &HashMap<String, BtDevice>,
    oui_db: Option<&OuiDb>,
    ble_db: Option<&BleCompanyDb>,
    names: &dyn Fn(&str) -> Option<String>,
    now: Instant,
) -> Vec<BtExport> {
    let mut out: Vec<BtExport> = devices
        .values()
        .map(|d| {
            let vendor = oui::vendor_for_bt(
                BtIdentity {
                    mac: &d.address,
                    is_random: d.is_random,
                    company_id: d.company_id,
                    icon: d.icon.as_deref(),
                },
                oui_db,
                ble_db,
            );
            let distance_m = d.ema.map(|ema| bt_distance(ema, d.tx_power));
            BtExport {
                address: d.address.clone(),
                name: d.name.clone(),
                vendor,
                icon: d.icon.clone(),
                company_id: d.company_id,
                is_random: d.is_random,
                rssi_dbm: d.rssi,
                rssi_avg_dbm: d.ema,
                tx_power: d.tx_power,
                distance_m,
                samples: d.sample_count,
                seen_secs_ago: now.duration_since(d.last_seen).as_secs(),
                tracked_secs: d.last_seen.duration_since(d.first_seen).as_secs(),
                custom_name: names(&d.address),
            }
        })
        .collect();
    // Sort by signal strength descending (strongest first)
    out.sort_by(|a, b| {
        b.rssi_dbm
            .unwrap_or(i16::MIN)
            .cmp(&a.rssi_dbm.unwrap_or(i16::MIN))
    });
    out
}

pub fn wifi_to_export(
    devices: &HashMap<String, WifiDevice>,
    oui_db: Option<&OuiDb>,
    names: &dyn Fn(&str) -> Option<String>,
    now: Instant,
) -> Vec<WifiExport> {
    let mut out: Vec<WifiExport> = devices
        .values()
        .map(|d| {
            let is_ap = d.kind == WifiKind::Ap;
            let vendor = oui::vendor_for_wifi(&d.mac, oui_db);
            let distance_m = match d.kind {
                WifiKind::Ap => d.ema.map(wifi_ap_distance),
                WifiKind::Client => d.ema.map(wifi_client_distance),
            };
            WifiExport {
                mac: d.mac.clone(),
                kind: if is_ap { "ap" } else { "client" },
                ssid: d.ssid.clone(),
                vendor,
                probing_for: d.probing_for.clone(),
                signal_pct: if is_ap { d.signal_pct } else { None },
                signal_pct_avg: if is_ap { d.ema } else { None },
                rssi_dbm: if is_ap { None } else { d.rssi },
                rssi_avg_dbm: if is_ap { None } else { d.ema },
                channel: d.channel.clone(),
                frequency: d.freq.clone(),
                security: d.security.clone(),
                distance_m,
                samples: d.sample_count,
                seen_secs_ago: now.duration_since(d.last_seen).as_secs(),
                tracked_secs: d.last_seen.duration_since(d.first_seen).as_secs(),
                custom_name: names(&d.mac),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        let a_sig = a.signal_pct.unwrap_or(0.0) + a.rssi_dbm.unwrap_or(-127) as f64;
        let b_sig = b.signal_pct.unwrap_or(0.0) + b.rssi_dbm.unwrap_or(-127) as f64;
        b_sig.partial_cmp(&a_sig).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

pub fn subghz_to_export(
    devices: &HashMap<String, SubGhzDevice>,
    names: &dyn Fn(&str) -> Option<String>,
    now: Instant,
) -> Vec<SubGhzExport> {
    let mut out: Vec<SubGhzExport> = devices
        .values()
        .map(|d| SubGhzExport {
            freq_hz: d.freq_hz,
            freq_mhz: d.freq_hz as f64 / 1_000_000.0,
            band: d.band.clone(),
            rssi_dbm: d.rssi,
            rssi_avg_dbm: d.ema,
            samples: d.sample_count,
            seen_secs_ago: now.duration_since(d.last_seen).as_secs(),
            tracked_secs: d.last_seen.duration_since(d.first_seen).as_secs(),
            custom_name: names(&d.freq_hz.to_string()),
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.rssi_dbm));
    out
}

/// Build a filesystem-safe export filename like `airprowl-bt-20260518-141233.json`.
/// Shells out to `date` for the timestamp (avoids pulling in chrono just for this).
pub fn export_filename(tab: &str) -> String {
    let ts = std::process::Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_default()
        });
    format!("airprowl-{}-{}.json", tab, ts)
}
