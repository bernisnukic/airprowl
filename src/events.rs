use crossterm::event::KeyEvent;

#[derive(Debug)]
pub enum AppEvent {
    BtUpdate(BtUpdate),
    BtError(String),
    WifiApUpdate(WifiApUpdate),
    WifiClientUpdate(WifiClientUpdate),
    WifiStatus(String),
    Key(KeyEvent),
    Tick,
}

#[derive(Debug, Default)]
pub struct BtUpdate {
    pub address: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub tx_power: Option<i16>,
}

#[derive(Debug)]
pub struct WifiApUpdate {
    pub bssid: String,
    pub ssid: Option<String>,
    pub signal_pct: Option<f64>,
    pub freq: Option<String>,
    pub channel: Option<String>,
    pub security: Option<String>,
}

#[derive(Debug)]
pub struct WifiClientUpdate {
    pub mac: String,
    pub rssi: Option<i16>,
    pub probing_for: Option<String>,
}
