use std::env;
use std::path::PathBuf;

pub const EMA_ALPHA: f64 = 0.4;
pub const BT_STALE_MS: u64 = 15_000;
pub const AP_STALE_MS: u64 = 30_000;
pub const CLIENT_STALE_MS: u64 = 15_000;
pub const DROP_MS: u64 = 120_000;
pub const BT_PREVEMA_EVERY: u32 = 5;
pub const AP_PREVEMA_EVERY: u32 = 3;
pub const CLIENT_PREVEMA_EVERY: u32 = 5;
pub const TICK_MS: u64 = 500;

pub struct Config {
    pub wifi_iface: String,
    pub mon_iface: String,
    pub bt_iface: Option<String>,
    pub names_path: PathBuf,
    pub is_root: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let args: Vec<String> = env::args().collect();

        if args.iter().any(|a| a == "--help" || a == "-h") {
            eprintln!("Usage: airprowl [OPTIONS]");
            eprintln!();
            eprintln!("Options:");
            eprintln!("  -h, --help  Show this help");
            eprintln!();
            eprintln!("Environment:");
            eprintln!("  WIFI_IFACE  WiFi interface (default: wlo1)");
            eprintln!("  BT_IFACE    Bluetooth adapter (default: auto-detect)");
            std::process::exit(0);
        }

        let wifi_iface = env::var("WIFI_IFACE").unwrap_or_else(|_| "wlo1".into());
        let mon_iface = format!("{}mon", wifi_iface);
        let bt_iface = env::var("BT_IFACE").ok();
        let names_path = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("names.json")))
            .unwrap_or_else(|| PathBuf::from("names.json"));
        let is_root = unsafe { libc::getuid() } == 0;

        Self {
            wifi_iface,
            mon_iface,
            bt_iface,
            names_path,
            is_root,
        }
    }
}
