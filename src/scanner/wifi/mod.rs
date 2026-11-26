#[cfg(target_os = "linux")]
pub mod ap_scanner;
#[cfg(target_os = "macos")]
#[path = "ap_scanner_macos.rs"]
pub mod ap_scanner;
#[cfg(target_os = "linux")]
pub mod client_sniffer;
#[cfg(target_os = "linux")]
pub mod monitor;

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::config::Config;
use crate::events::AppEvent;

/// Passive WiFi client discovery: puts the card in monitor mode and sniffs
/// probe/data frames until the event channel closes. Best effort: when monitor
/// mode is unavailable it reports why and returns, leaving AP scanning alone.
#[cfg(target_os = "linux")]
pub async fn run_client_capture(config: Arc<Config>, tx: mpsc::Sender<AppEvent>) {
    use std::collections::HashSet;
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    let mon_handle = match monitor::setup_monitor(config).await {
        Ok(handle) => handle,
        Err(e) => {
            let msg = format!("No monitor mode — {}. AP scanning only.", e);
            tx.send(AppEvent::WifiStatus(Some(msg))).await.ok();
            return;
        }
    };

    let known_aps = Arc::new(Mutex::new(HashSet::new()));
    let cancel = CancellationToken::new();
    let sniff_iface = mon_handle.sniff_iface.clone();
    if let Err(e) =
        client_sniffer::run_client_sniffer(sniff_iface, tx.clone(), known_aps, cancel.clone()).await
    {
        tx.send(AppEvent::WifiStatus(Some(format!("Sniffer error: {}", e))))
            .await
            .ok();
    }
    cancel.cancel();
    drop(mon_handle);
}

/// macOS gives no supported way to put the WiFi card in monitor mode, so
/// client sniffing is unavailable there.
#[cfg(target_os = "macos")]
pub async fn run_client_capture(_config: Arc<Config>, _tx: mpsc::Sender<AppEvent>) {}
