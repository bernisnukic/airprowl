use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::process::Command;

use crate::config::{self, Config};
use crate::events::{AppEvent, WifiApUpdate};

/// Scan APs using nmcli (works without root). Exits cleanly when `tx` closes.
pub async fn run_ap_scanner(
    config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    let mut last_iface_problem: Option<String> = None;

    loop {
        let problem = check_wifi_iface(&config.wifi_iface).await;
        if problem != last_iface_problem {
            // Send the new state (Some = warning, None = clear).
            // None matters: it wipes a stale "DOWN" warning when the iface comes up.
            if tx.send(AppEvent::WifiStatus(problem.clone())).await.is_err() {
                return Ok(());
            }
            last_iface_problem = problem.clone();
        }

        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "rescan"])
            .output()
            .await;

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            _ = tx.closed() => return Ok(()),
        }

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
                        if tx.send(AppEvent::WifiApUpdate(update)).await.is_err() {
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                if tx.send(AppEvent::WifiStatus(Some(format!("nmcli error: {}", e))))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(config::AP_SCAN_INTERVAL_MS)) => {}
            _ = tx.closed() => return Ok(()),
        }
    }
}

/// Returns Some(diagnostic) if the WiFi interface is missing or down,
/// otherwise None. Reads /sys to avoid shelling out.
async fn check_wifi_iface(iface: &str) -> Option<String> {
    let path = format!("/sys/class/net/{}/operstate", iface);
    match tokio::fs::read_to_string(&path).await {
        Ok(state) => {
            if state.trim() == "down" {
                Some(format!(
                    "WiFi iface {} is DOWN — run: sudo ip link set {} up",
                    iface, iface
                ))
            } else {
                None
            }
        }
        Err(_) => Some(format!(
            "WiFi iface {} not found — set WIFI_IFACE=<name> (see: ip -br link)",
            iface
        )),
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
