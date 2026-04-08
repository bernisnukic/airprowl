use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::process::Command;

use crate::config::Config;
use crate::events::{AppEvent, WifiApUpdate};

/// Scan APs using nmcli (works without root).
/// Falls back gracefully if nmcli is not available.
pub async fn run_ap_scanner(
    _config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    loop {
        // Trigger rescan
        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "rescan"])
            .output()
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // List APs
        match Command::new("nmcli")
            .args(["-t", "-f", "SSID,BSSID,SIGNAL,FREQ,CHAN,SECURITY,MODE", "dev", "wifi", "list"])
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(update) = parse_nmcli_line(line) {
                        tx.send(AppEvent::WifiApUpdate(update)).await.ok();
                    }
                }
            }
            Err(e) => {
                tx.send(AppEvent::WifiStatus(format!("nmcli error: {}", e)))
                    .await
                    .ok();
            }
        }

        // Wait before next scan cycle
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

/// Parse nmcli -t output line with escaped colons.
fn parse_nmcli_line(line: &str) -> Option<WifiApUpdate> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == ':' {
            current.push(':');
            i += 2;
        } else if chars[i] == ':' {
            parts.push(std::mem::take(&mut current));
            i += 1;
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }
    parts.push(current);

    if parts.len() < 6 {
        return None;
    }

    let signal: f64 = parts[2].parse().ok()?;

    Some(WifiApUpdate {
        bssid: parts[1].clone(),
        ssid: if parts[0].is_empty() {
            Some("(hidden)".into())
        } else {
            Some(parts[0].clone())
        },
        signal_pct: Some(signal),
        freq: Some(parts[3].clone()),
        channel: Some(parts[4].clone()),
        security: Some(parts[5].clone()),
    })
}
