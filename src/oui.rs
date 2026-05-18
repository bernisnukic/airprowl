use std::collections::HashMap;

use crate::ble::BleCompanyDb;

/// IEEE-derived OUI database (sourced from nmap-mac-prefixes). Embedded at
/// build time so airprowl needs no external file or network at runtime.
/// To refresh, replace `data/oui-prefixes.txt` with a newer copy.
const EMBEDDED_OUI: &str = include_str!("../data/oui-prefixes.txt");

pub struct OuiDb {
    map: HashMap<[u8; 3], String>,
}

impl OuiDb {
    pub fn load() -> Option<Self> {
        let mut map = HashMap::with_capacity(50_000);
        for line in EMBEDDED_OUI.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((prefix, vendor)) = parse_line(line) {
                map.entry(prefix).or_insert(vendor);
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(Self { map })
        }
    }

    pub fn lookup(&self, mac: &str) -> Option<&str> {
        let prefix = parse_mac_prefix(mac)?;
        self.map.get(&prefix).map(|s| s.as_str())
    }
}

/// Format-agnostic: extracts first 3 bytes from any of "001122", "00:11:22",
/// "00-11-22", or "00.11.22".
fn parse_mac_prefix(mac: &str) -> Option<[u8; 3]> {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() < 6 {
        return None;
    }
    let bytes = hex.as_bytes();
    Some([
        u8::from_str_radix(std::str::from_utf8(&bytes[0..2]).ok()?, 16).ok()?,
        u8::from_str_radix(std::str::from_utf8(&bytes[2..4]).ok()?, 16).ok()?,
        u8::from_str_radix(std::str::from_utf8(&bytes[4..6]).ok()?, 16).ok()?,
    ])
}

fn parse_line(line: &str) -> Option<([u8; 3], String)> {
    let mut tokens = line.split_whitespace();
    let prefix = parse_mac_prefix(tokens.next()?)?;
    let rest: String = tokens.collect::<Vec<_>>().join(" ");
    // hwdata markers, then comments
    let cleaned = rest.replace("(hex)", "").replace("(base 16)", "");
    let vendor = cleaned.split('#').next()?.trim().to_string();
    if vendor.is_empty() {
        None
    } else {
        Some((prefix, vendor))
    }
}

/// Locally-administered bit (bit 1 of first byte). The 802.11 randomization
/// signal used by modern iOS/Android/Windows for probing privacy.
fn is_locally_administered(mac: &str) -> bool {
    parse_mac_prefix(mac).is_some_and(|b| b[0] & 0x02 != 0)
}

/// WiFi-side lookup: detects randomness via the IEEE LAA bit.
pub fn vendor_for_wifi(mac: &str, db: Option<&OuiDb>) -> Option<String> {
    if is_locally_administered(mac) {
        return Some("random".to_string());
    }
    db?.lookup(mac).map(|s| s.to_string())
}

/// All the BLE signals we can use to identify a device, in order of preference.
pub struct BtIdentity<'a> {
    pub mac: &'a str,
    pub is_random: bool,
    pub company_id: Option<u16>,
    pub icon: Option<&'a str>,
}

/// BT-side lookup combining every available signal: BlueZ-reported address
/// type, OUI database, BLE Company Identifier from advertising manufacturer
/// data, and BlueZ's heuristic icon hint (e.g. "phone", "audio-card").
/// Returns a compact label like "Apple, phone" or "Bose, audio-headset", or
/// "random" when nothing identifying is broadcast.
pub fn vendor_for_bt(
    id: BtIdentity<'_>,
    oui_db: Option<&OuiDb>,
    ble_db: Option<&BleCompanyDb>,
) -> Option<String> {
    let vendor_full: Option<String> = (!id.is_random)
        .then(|| oui_db.and_then(|db| db.lookup(id.mac)).map(String::from))
        .flatten()
        .or_else(|| {
            id.company_id
                .and_then(|cid| ble_db.and_then(|db| db.lookup(cid)))
                .map(String::from)
        });

    let vendor = vendor_full.as_deref().map(shorten_vendor);
    let icon = id.icon.filter(|s| !s.is_empty());

    match (vendor.as_deref(), icon) {
        (Some(v), Some(i)) => Some(format!("{}, {}", v, i)),
        (Some(v), None) => Some(v.to_string()),
        (None, Some(i)) => Some(i.to_string()),
        (None, None) if id.is_random => Some("random".to_string()),
        (None, None) => None,
    }
}

/// "Samsung Electronics Co. Ltd." → "Samsung", "Apple, Inc." → "Apple".
/// Trims corporate suffixes that consume column width without adding signal.
fn shorten_vendor(name: &str) -> String {
    if let Some(idx) = name.find(',') {
        return name[..idx].trim().to_string();
    }
    const STOP: &[&str] = &[
        "Inc.", "Inc", "Corp.", "Corp", "LLC", "Corporation", "Ltd.", "Ltd",
        "Co.", "Co", "Electronics", "Limited", "GmbH", "S.A.", "SA", "BV",
        "AG", "AS", "AB", "OY", "S.r.l.", "S.p.A.", "PLC", "B.V.",
    ];
    let words: Vec<&str> = name
        .split_whitespace()
        .take_while(|w| !STOP.contains(w))
        .collect();
    if words.is_empty() {
        name.to_string()
    } else {
        words.join(" ")
    }
}
