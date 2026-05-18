use std::collections::HashMap;
use std::time::Instant;

use crate::config;
use crate::events::{BtUpdate, SubGhzSignal, WifiApUpdate, WifiClientUpdate};
use crate::signal::ema_update;

#[derive(Debug, Clone)]
pub struct BtDevice {
    pub address: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub tx_power: Option<i16>,
    pub ema: Option<f64>,
    pub prev_ema: Option<f64>,
    pub sample_count: u32,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub is_random: bool,
    pub icon: Option<String>,
    pub company_id: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WifiKind {
    Ap,
    Client,
}

#[derive(Debug, Clone)]
pub struct SubGhzDevice {
    pub freq_hz: u32,
    pub rssi: i16,
    pub ema: Option<f64>,
    pub prev_ema: Option<f64>,
    pub sample_count: u32,
    pub band: String,
    pub first_seen: Instant,
    pub last_seen: Instant,
}

#[derive(Debug, Clone)]
pub struct WifiDevice {
    pub mac: String,
    pub kind: WifiKind,
    pub ssid: Option<String>,
    pub freq: Option<String>,
    pub channel: Option<String>,
    pub security: Option<String>,
    pub signal_pct: Option<f64>,
    pub probing_for: Option<String>,
    pub rssi: Option<i16>,
    pub ema: Option<f64>,
    pub prev_ema: Option<f64>,
    pub sample_count: u32,
    pub first_seen: Instant,
    pub last_seen: Instant,
}

pub fn apply_bt_update(devices: &mut HashMap<String, BtDevice>, update: BtUpdate) {
    let now = Instant::now();
    let device = devices.entry(update.address.clone()).or_insert_with(|| BtDevice {
        address: update.address,
        name: None,
        rssi: None,
        tx_power: None,
        ema: None,
        prev_ema: None,
        sample_count: 0,
        first_seen: now,
        last_seen: now,
        is_random: false,
        icon: None,
        company_id: None,
    });

    device.last_seen = now;
    if update.is_random {
        device.is_random = true;
    }
    if update.icon.is_some() {
        device.icon = update.icon;
    }
    if update.company_id.is_some() {
        device.company_id = update.company_id;
    }

    if let Some(name) = update.name {
        device.name = Some(name);
    }

    if let Some(rssi) = update.rssi {
        device.rssi = Some(rssi);
        device.sample_count += 1;

        match device.ema {
            None => {
                device.ema = Some(rssi as f64);
                device.prev_ema = None;
            }
            Some(prev) => {
                if device.sample_count % config::BT_PREVEMA_EVERY == 0 {
                    device.prev_ema = Some(prev);
                }
                device.ema = Some(ema_update(prev, rssi as f64));
            }
        }
    }

    if let Some(tp) = update.tx_power {
        device.tx_power = Some(tp);
    }
}

pub fn apply_wifi_ap_update(devices: &mut HashMap<String, WifiDevice>, update: WifiApUpdate) {
    let now = Instant::now();
    let device = devices.entry(update.bssid.clone()).or_insert_with(|| WifiDevice {
        mac: update.bssid,
        kind: WifiKind::Ap,
        ssid: None,
        freq: None,
        channel: None,
        security: None,
        signal_pct: None,
        probing_for: None,
        rssi: None,
        ema: None,
        prev_ema: None,
        sample_count: 0,
        first_seen: now,
        last_seen: now,
    });

    device.last_seen = now;
    device.kind = WifiKind::Ap;
    device.ssid = update.ssid;
    device.freq = update.freq;
    device.channel = update.channel;
    device.security = update.security;

    if let Some(sig) = update.signal_pct {
        device.signal_pct = Some(sig);
        device.sample_count += 1;

        match device.ema {
            None => {
                device.ema = Some(sig);
                device.prev_ema = None;
            }
            Some(prev) => {
                if device.sample_count % config::AP_PREVEMA_EVERY == 0 {
                    device.prev_ema = Some(prev);
                }
                device.ema = Some(ema_update(prev, sig));
            }
        }
    }
}

pub fn apply_subghz_update(devices: &mut HashMap<String, SubGhzDevice>, sig: SubGhzSignal) {
    let now = Instant::now();
    let key = format!("{}", sig.freq_hz);
    let device = devices.entry(key).or_insert_with(|| SubGhzDevice {
        freq_hz: sig.freq_hz,
        rssi: sig.rssi_dbm,
        ema: None,
        prev_ema: None,
        sample_count: 0,
        band: sig.band.clone(),
        first_seen: now,
        last_seen: now,
    });

    device.last_seen = now;
    device.rssi = sig.rssi_dbm;
    device.sample_count += 1;

    match device.ema {
        None => {
            device.ema = Some(sig.rssi_dbm as f64);
            device.prev_ema = None;
        }
        Some(prev) => {
            if device.sample_count % config::SUBGHZ_PREVEMA_EVERY == 0 {
                device.prev_ema = Some(prev);
            }
            device.ema = Some(ema_update(prev, sig.rssi_dbm as f64));
        }
    }
}

pub fn apply_wifi_client_update(devices: &mut HashMap<String, WifiDevice>, update: WifiClientUpdate) {
    let now = Instant::now();
    let device = devices.entry(update.mac.clone()).or_insert_with(|| WifiDevice {
        mac: update.mac,
        kind: WifiKind::Client,
        ssid: None,
        freq: None,
        channel: None,
        security: None,
        signal_pct: None,
        probing_for: None,
        rssi: None,
        ema: None,
        prev_ema: None,
        sample_count: 0,
        first_seen: now,
        last_seen: now,
    });

    device.last_seen = now;
    device.kind = WifiKind::Client;

    if let Some(ssid) = update.probing_for {
        device.probing_for = Some(ssid);
    }

    if let Some(rssi) = update.rssi {
        device.rssi = Some(rssi);
        device.sample_count += 1;

        match device.ema {
            None => {
                device.ema = Some(rssi as f64);
                device.prev_ema = None;
            }
            Some(prev) => {
                if device.sample_count % config::CLIENT_PREVEMA_EVERY == 0 {
                    device.prev_ema = Some(prev);
                }
                device.ema = Some(ema_update(prev, rssi as f64));
            }
        }
    }
}
