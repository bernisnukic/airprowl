// A few TUI event-handler arms could become match guards (clippy::collapsible_match),
// but that changes fall-through semantics — deferred. The rest of clippy is enforced.
#![allow(clippy::collapsible_match)]

mod app;
mod ble;
mod cli;
mod config;
mod demo;
mod events;
mod export;
#[cfg(target_os = "macos")]
mod macos_spawn;
mod names;
mod oui;
mod scanner;
mod signal;
mod store;
mod tui;
mod util;

use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::events::AppEvent;

fn main() -> anyhow::Result<()> {
    // Before any threads exist: may relaunch this binary and never return.
    #[cfg(target_os = "macos")]
    macos_spawn::become_responsible();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    // A backend stuck in a blocking OS call (CoreBluetooth waiting on a
    // permission prompt, say) must not keep the process alive after we finish.
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

async fn run() -> anyhow::Result<()> {
    let config = Arc::new(Config::from_env());

    if config.cli.is_some() {
        return cli::run(config).await;
    }

    let (tx, rx) = mpsc::channel::<AppEvent>(1024);

    if config.demo {
        // Simulated device feed — no hardware needed (drives the README demo).
        let demo_tx = tx.clone();
        tokio::spawn(async move { demo::run_demo(demo_tx).await });
    } else {
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
                .send(AppEvent::WifiStatus(Some(format!("AP scanner error: {}", e))))
                .await
                .ok();
        }
    });

    // Try monitor mode for passive client sniffing (on connected channel)
    tokio::spawn(scanner::wifi::run_client_capture(config.clone(), tx.clone()));

    // Spawn Sub-GHz scanner (Yard Stick One)
    let subghz_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = scanner::subghz::yardstick::run_subghz_scanner(subghz_tx.clone()).await {
            subghz_tx
                .send(AppEvent::SubGhzStatus(format!("Sub-GHz error: {}", e)))
                .await
                .ok();
        }
    });
    } // real scanners (skipped under --demo)

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
            if let Event::Key(key) = event
                && key_tx.send(AppEvent::Key(key)).await.is_err()
            {
                break;
            }
        }
    });

    // Setup terminal and run
    let mut terminal = tui::setup_terminal()?;
    let mut app = app::App::new(config, tx.clone(), rx);

    let result = app.run(&mut terminal).await;

    tui::restore_terminal(&mut terminal)?;

    // Close the channel so scanners exit via `tx.closed()` / `tx.is_closed()`,
    // letting Drop impls (USB release, monitor-iface teardown) run.
    drop(tx);
    drop(app);

    // Grace period for blocking-task cleanup (USB reset, `iw dev … del`).
    tokio::time::sleep(Duration::from_millis(config::SHUTDOWN_GRACE_MS)).await;

    result
}
