use std::collections::HashMap;

/// Bluetooth SIG company identifiers, embedded at build time.
/// Source: https://bitbucket.org/bluetooth-SIG/public/src/main/assigned_numbers/company_identifiers/
const EMBEDDED: &str = include_str!("../data/ble-companies.txt");

pub struct BleCompanyDb {
    map: HashMap<u16, String>,
}

impl BleCompanyDb {
    pub fn load() -> Option<Self> {
        let mut map = HashMap::with_capacity(4096);
        for line in EMBEDDED.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(2, ' ');
            let Some(id_str) = parts.next() else { continue };
            let Some(name) = parts.next() else { continue };
            let Ok(id) = u16::from_str_radix(id_str, 16) else { continue };
            map.insert(id, name.trim().to_string());
        }
        if map.is_empty() {
            None
        } else {
            Some(Self { map })
        }
    }

    pub fn lookup(&self, id: u16) -> Option<&str> {
        self.map.get(&id).map(|s| s.as_str())
    }
}
