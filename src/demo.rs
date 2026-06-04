//! Simulated scanner feed for `--demo`. Pushes realistic, gently-oscillating
//! Bluetooth / WiFi / Sub-GHz events into the same channel the real scanners
//! use, so the TUI looks alive with no hardware (drives the README recording).

use std::time::Duration;
use tokio::sync::mpsc;

use crate::events::{AppEvent, BtUpdate, SubGhzSignal, WifiApUpdate, WifiClientUpdate};

type BtRow = (&'static str, Option<&'static str>, Option<u16>, Option<&'static str>, bool, i16);
type ApRow = (&'static str, Option<&'static str>, &'static str, &'static str, &'static str, f64);

// address, name, company_id, icon, is_random, base rssi (dBm)
const BT: &[BtRow] = &[
    ("F4:0E:22:6B:1A:3C", Some("AirPods Pro"), Some(76), Some("audio-headset"), false, -47),
    ("5C:F3:70:22:9D:18", None, Some(76), Some("phone"), false, -61),
    ("C8:69:CD:04:7E:B2", Some("Living Room TV"), Some(117), Some("video-display"), false, -68),
    ("48:A6:B8:1F:0C:55", Some("Pixel Buds"), Some(224), Some("audio-headset"), false, -73),
    ("1C:36:BB:9A:55:20", None, None, Some("computer"), false, -79),
    ("7E:A1:33:9F:42:0B", None, None, None, true, -85),
];

// bssid, ssid, security, channel, frequency, base signal (%)
const APS: &[ApRow] = &[
    ("A4:2B:B0:8C:11:90", Some("Aurora_5G"), "WPA2", "44", "5220 MHz", 84.0),
    ("D8:47:32:6E:AA:01", Some("eduroam"), "WPA2-EAP", "36", "5180 MHz", 61.0),
    ("F0:9F:C2:1D:55:7A", Some("CoffeeBar Guest"), "WPA2", "6", "2437 MHz", 48.0),
    ("2C:30:33:90:E1:44", None, "WPA3", "11", "2462 MHz", 33.0),
];

// mac, probing_for, base rssi (dBm)
const CLIENTS: &[(&str, Option<&str>, i16)] = &[
    ("DA:A1:19:7C:08:2E", Some("Aurora_5G"), -57),
    ("9E:62:01:B4:7F:13", None, -66),
    ("46:8B:2D:5A:90:C1", Some("xfinitywifi"), -78),
];

// freq_hz, band, base rssi (dBm)
const SUBGHZ: &[(u32, &str, i16)] = &[
    (433_920_000, "433 MHz", -61),
    (315_000_000, "315 MHz", -77),
    (868_300_000, "868 MHz", -69),
    (915_000_000, "915 MHz", -64),
];

pub async fn run_demo(tx: mpsc::Sender<AppEvent>) {
    tx.send(AppEvent::SubGhzStatus("demo mode — simulated devices".into()))
        .await
        .ok();

    let mut tick: u64 = 0;
    loop {
        if tx.is_closed() {
            return;
        }
        let t = tick as f64 * 0.45;
        let wobble = |phase: f64| ((t + phase).sin() * 4.0) as i16;

        for (i, &(address, name, company_id, icon, is_random, base)) in BT.iter().enumerate() {
            tx.send(AppEvent::BtUpdate(BtUpdate {
                address: address.to_string(),
                name: name.map(String::from),
                rssi: Some(base + wobble(i as f64)),
                tx_power: Some(-59),
                is_random,
                icon: icon.map(String::from),
                company_id,
            }))
            .await
            .ok();
        }
        for (i, &(bssid, ssid, security, channel, freq, base)) in APS.iter().enumerate() {
            tx.send(AppEvent::WifiApUpdate(WifiApUpdate {
                bssid: bssid.to_string(),
                ssid: ssid.map(String::from),
                signal_pct: Some((base + (t + i as f64).sin() * 5.0).clamp(0.0, 100.0)),
                freq: Some(freq.to_string()),
                channel: Some(channel.to_string()),
                security: Some(security.to_string()),
            }))
            .await
            .ok();
        }
        for (i, &(mac, probing_for, base)) in CLIENTS.iter().enumerate() {
            tx.send(AppEvent::WifiClientUpdate(WifiClientUpdate {
                mac: mac.to_string(),
                rssi: Some(base + wobble(i as f64 + 2.0)),
                probing_for: probing_for.map(String::from),
            }))
            .await
            .ok();
        }
        for (i, &(freq_hz, band, base)) in SUBGHZ.iter().enumerate() {
            tx.send(AppEvent::SubGhzSignal(SubGhzSignal {
                freq_hz,
                rssi_dbm: base + wobble(i as f64 + 4.0),
                band: band.to_string(),
            }))
            .await
            .ok();
        }

        tick += 1;
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
}
