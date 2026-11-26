//! WiFi access-point scanning on macOS.
//!
//! Reads `system_profiler SPAirPortDataType -json`, which lists the network
//! the Mac is joined to plus the other networks it can hear, with channel,
//! security and signal, without root.
//!
//! Two macOS limits shape the result:
//! - No BSSIDs, so each network is keyed "ssid@channel" in place of a MAC,
//!   and access points sharing an SSID on one channel show as one row.
//! - Recent macOS replaces every network name with "<redacted>" for tools
//!   like this one (CoreWLAN does the same, even with Location access), so
//!   rows are then keyed by channel alone and the status line says why.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::config::{self, Config};
use crate::events::{AppEvent, WifiApUpdate};

/// What macOS puts in place of a network name it will not reveal.
const REDACTED: &str = "<redacted>";
const NAMES_HIDDEN: &str = "macOS hides WiFi network names from airprowl; channel, signal and security are still shown";

/// Scan APs via system_profiler. Exits cleanly when `tx` closes.
pub async fn run_ap_scanner(
    config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    let mut last_problem: Option<String> = None;

    loop {
        let (problem, networks) = match scan().await {
            Ok(json) => parse_scan(&json, &config.wifi_iface),
            Err(e) => (Some(format!("system_profiler failed: {}", e)), Vec::new()),
        };

        if problem != last_problem {
            // None clears a stale warning once the interface is usable again.
            if tx.send(AppEvent::WifiStatus(problem.clone())).await.is_err() {
                return Ok(());
            }
            last_problem = problem;
        }

        for update in networks {
            if tx.send(AppEvent::WifiApUpdate(update)).await.is_err() {
                return Ok(());
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(config::AP_SCAN_INTERVAL_MS)) => {}
            _ = tx.closed() => return Ok(()),
        }
    }
}

async fn scan() -> anyhow::Result<Value> {
    // The default detail level: "basic" and "mini" leave out nearby networks.
    let out = Command::new("system_profiler")
        .args(["SPAirPortDataType", "-json"])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("exit status {}", out.status);
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// Pick the interface and turn its network lists into updates. Returns a
/// warning when there is nothing to scan with.
fn parse_scan(json: &Value, iface: &str) -> (Option<String>, Vec<WifiApUpdate>) {
    let interfaces = json["SPAirPortDataType"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["spairport_airport_interfaces"].as_array())
        .flatten()
        .collect::<Vec<_>>();

    let Some(card) = interfaces
        .iter()
        .find(|i| i["_name"].as_str() == Some(iface))
        .or(interfaces.first())
    else {
        return (Some("No WiFi interface found".into()), Vec::new());
    };
    let name = card["_name"].as_str().unwrap_or(iface);

    if card["spairport_status_information"].as_str() == Some("spairport_status_off") {
        let msg = format!("WiFi ({}) is off — turn it on in Control Center", name);
        return (Some(msg), Vec::new());
    }

    let current = card.get("spairport_current_network_information");
    let others = card["spairport_airport_other_local_wireless_networks"]
        .as_array()
        .into_iter()
        .flatten();
    // Mesh networks put several access points under one SSID on one channel.
    // With no BSSID to tell them apart they share a row, so report only the
    // strongest; feeding all of them in would make the row's signal jump.
    let all: Vec<&Value> = current.into_iter().chain(others).collect();
    let redacted = all.iter().any(|n| n["_name"].as_str() == Some(REDACTED));
    let mut networks: Vec<WifiApUpdate> = Vec::new();
    for net in all.into_iter().filter_map(parse_network) {
        match networks.iter_mut().find(|n| n.bssid == net.bssid) {
            Some(seen) if net.signal_pct > seen.signal_pct => *seen = net,
            Some(_) => {}
            None => networks.push(net),
        }
    }
    (redacted.then(|| NAMES_HIDDEN.to_string()), networks)
}

fn parse_network(net: &Value) -> Option<WifiApUpdate> {
    let ssid = net["_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(hidden)")
        .to_string();

    // "149 (5GHz, 80MHz)" on current macOS; a bare number on older releases.
    let channel_field = match &net["spairport_network_channel"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let channel: u32 = channel_field.split_whitespace().next()?.parse().ok()?;
    let band = channel_field
        .split(['(', ',', ')'])
        .map(str::trim)
        .find(|part| part.ends_with("GHz"))
        .unwrap_or("");

    // "-61 dBm / -94 dBm" is signal / noise.
    let rssi: Option<i32> = net["spairport_signal_noise"]
        .as_str()
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok());

    let (key, ssid) = if ssid == REDACTED {
        (format!("ch{}", channel), "(name hidden by macOS)".to_string())
    } else {
        (format!("{}@{}", ssid, channel), ssid)
    };

    Some(WifiApUpdate {
        bssid: key,
        ssid: Some(ssid),
        signal_pct: rssi.map(rssi_to_pct),
        freq: channel_mhz(channel, band).map(|mhz| format!("{} MHz", mhz)),
        channel: Some(channel.to_string()),
        security: net["spairport_security_mode"].as_str().map(security_label),
    })
}

/// NetworkManager's mapping, so bars and colors match the Linux build.
fn rssi_to_pct(rssi: i32) -> f64 {
    (2 * (rssi + 100)).clamp(0, 100) as f64
}

fn channel_mhz(channel: u32, band: &str) -> Option<u32> {
    match (band, channel) {
        ("6GHz", 1..=233) => Some(5950 + 5 * channel),
        ("5GHz", _) | ("", 32..=177) => Some(5000 + 5 * channel),
        (_, 14) => Some(2484),
        (_, 1..=13) => Some(2407 + 5 * channel),
        _ => None,
    }
}

/// "spairport_security_mode_wpa2_personal" -> "WPA2", "..._none" -> "" (open).
fn security_label(mode: &str) -> String {
    let mode = mode.trim_start_matches("spairport_security_mode_");
    if mode == "none" {
        return String::new();
    }
    mode.replace("_personal_mixed", "")
        .replace("_personal", "")
        .replace("_enterprise", " 802.1X")
        .replace('_', "/")
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "SPAirPortDataType": [{
                "spairport_airport_interfaces": [{
                    "_name": "en0",
                    "spairport_status_information": "spairport_status_connected",
                    "spairport_current_network_information": {
                        "_name": "Home",
                        "spairport_network_channel": "149 (5GHz, 80MHz)",
                        "spairport_security_mode": "spairport_security_mode_wpa2_personal",
                        "spairport_signal_noise": "-52 dBm / -93 dBm"
                    },
                    "spairport_airport_other_local_wireless_networks": [
                        {
                            "_name": "Cafe",
                            "spairport_network_channel": "6 (2GHz, 20MHz)",
                            "spairport_security_mode": "spairport_security_mode_none",
                            "spairport_signal_noise": "-81 dBm / -95 dBm"
                        },
                        {
                            "spairport_network_channel": 37,
                            "spairport_security_mode": "spairport_security_mode_wpa3_personal"
                        }
                    ]
                }]
            }]
        })
    }

    #[test]
    fn reads_current_and_nearby_networks() {
        let (problem, nets) = parse_scan(&sample(), "en0");
        assert_eq!(problem, None);
        assert_eq!(nets.len(), 3);

        assert_eq!(nets[0].bssid, "Home@149");
        assert_eq!(nets[0].signal_pct, Some(96.0));
        assert_eq!(nets[0].freq.as_deref(), Some("5745 MHz"));
        assert_eq!(nets[0].security.as_deref(), Some("WPA2"));

        assert_eq!(nets[1].freq.as_deref(), Some("2437 MHz"));
        assert_eq!(nets[1].security.as_deref(), Some("")); // open network

        assert_eq!(nets[2].ssid.as_deref(), Some("(hidden)"));
        assert_eq!(nets[2].signal_pct, None);
        assert_eq!(nets[2].freq.as_deref(), Some("5185 MHz"));
    }

    #[test]
    fn mesh_nodes_on_one_channel_keep_the_strongest() {
        let node = |signal: &str| serde_json::json!({
            "_name": "Mesh",
            "spairport_network_channel": "6 (2GHz, 20MHz)",
            "spairport_signal_noise": signal
        });
        let json = serde_json::json!({"SPAirPortDataType": [{"spairport_airport_interfaces": [{
            "_name": "en0",
            "spairport_airport_other_local_wireless_networks":
                [node("-68 dBm / -81 dBm"), node("-48 dBm / -81 dBm"), node("-75 dBm / -81 dBm")]
        }]}]});
        let (_, nets) = parse_scan(&json, "en0");
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].signal_pct, Some(100.0)); // the -48 dBm node
    }

    #[test]
    fn redacted_names_are_keyed_by_channel_and_flagged() {
        let json = serde_json::json!({"SPAirPortDataType": [{"spairport_airport_interfaces": [{
            "_name": "en0",
            "spairport_airport_other_local_wireless_networks": [
                {"_name": "<redacted>", "spairport_network_channel": "6 (2GHz, 20MHz)"},
                {"_name": "<redacted>", "spairport_network_channel": "44 (5GHz, 40MHz)"}
            ]
        }]}]});
        let (problem, nets) = parse_scan(&json, "en0");
        assert_eq!(problem.as_deref(), Some(NAMES_HIDDEN));
        assert_eq!(nets[0].bssid, "ch6");
        assert_eq!(nets[1].bssid, "ch44");
        assert_eq!(nets[0].ssid.as_deref(), Some("(name hidden by macOS)"));
    }

    #[test]
    fn reports_wifi_off() {
        let json = serde_json::json!({"SPAirPortDataType": [{"spairport_airport_interfaces": [
            {"_name": "en1", "spairport_status_information": "spairport_status_off"}
        ]}]});
        let (problem, nets) = parse_scan(&json, "en0");
        assert!(problem.unwrap().contains("off"));
        assert!(nets.is_empty());
    }

    #[test]
    fn maps_signal_and_security() {
        assert_eq!(rssi_to_pct(-40), 100.0);
        assert_eq!(rssi_to_pct(-75), 50.0);
        assert_eq!(rssi_to_pct(-110), 0.0);
        assert_eq!(security_label("spairport_security_mode_wpa2_enterprise"), "WPA2 802.1X");
        assert_eq!(security_label("spairport_security_mode_wpa2_wpa3_personal"), "WPA2/WPA3");
    }
}
