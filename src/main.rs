mod app;
mod config;
mod events;
mod names;
mod scanner;
mod signal;
mod store;
mod tui;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::events::AppEvent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Arc::new(Config::from_env());
    let (tx, rx) = mpsc::channel::<AppEvent>(1024);

    // Spawn Bluetooth scanner
    let bt_tx = tx.clone();
    let bt_cfg = config.clone();
    tokio::spawn(async move {
        if let Err(e) = scanner::bluetooth::run_bt_scanner(bt_cfg, bt_tx.clone()).await {
            bt_tx
                .send(AppEvent::BtError(format!("{}", e)))
                .await
                .ok();
        }
    });

    // Spawn WiFi AP scanner
    let ap_tx = tx.clone();
    let ap_cfg = config.clone();
    tokio::spawn(async move {
        if let Err(e) = scanner::wifi::ap_scanner::run_ap_scanner(ap_cfg, ap_tx.clone()).await {
            ap_tx
                .send(AppEvent::WifiStatus(format!("AP scanner error: {}", e)))
                .await
                .ok();
        }
    });

    // Try monitor mode for passive client sniffing (on connected channel)
    let mon_tx = tx.clone();
    let mon_cfg = config.clone();
    tokio::spawn(async move {
        match scanner::wifi::monitor::setup_monitor(mon_cfg.clone()).await {
            Err(e) => {
                mon_tx
                    .send(AppEvent::WifiStatus(format!(
                        "No monitor mode — {}. AP scanning only.",
                        e
                    )))
                    .await
                    .ok();
            }
            Ok(mon_handle) => {
                let known_aps = Arc::new(Mutex::new(HashSet::new()));
                let cancel = CancellationToken::new();

                let sniff_iface = mon_handle.sniff_iface.clone();
                if let Err(e) = scanner::wifi::client_sniffer::run_client_sniffer(
                    sniff_iface,
                    mon_tx.clone(),
                    known_aps,
                    cancel.clone(),
                )
                .await
                {
                    mon_tx
                        .send(AppEvent::WifiStatus(format!("Sniffer error: {}", e)))
                        .await
                        .ok();
                }

                cancel.cancel();
                drop(mon_handle);
            }
        }
    });

    // Tick timer (500ms)
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(config::TICK_MS));
        loop {
            interval.tick().await;
            if tick_tx.send(AppEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    // Keyboard event pump
    let key_tx = tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        while let Some(Ok(event)) = reader.next().await {
            if let Event::Key(key) = event {
                if key_tx.send(AppEvent::Key(key)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Setup terminal and run
    let mut terminal = tui::setup_terminal()?;
    let mut app = app::App::new(config, rx);

    let result = app.run(&mut terminal).await;

    tui::restore_terminal(&mut terminal)?;

    result
}
