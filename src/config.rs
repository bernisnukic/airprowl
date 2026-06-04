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
pub const SUBGHZ_PREVEMA_EVERY: u32 = 5;
pub const TICK_MS: u64 = 500;
pub const AP_SCAN_INTERVAL_MS: u64 = 10_000;
pub const SUBGHZ_STEP_DELAY_US: u64 = 500;
pub const SHUTDOWN_GRACE_MS: u64 = 200;
pub const DEFAULT_CLI_DURATION_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScanTarget {
    Bt,
    Wifi,
    SubGhz,
}

#[derive(Debug, Clone)]
pub struct CliMode {
    pub scan: ScanTarget,
    pub duration_secs: u64,
}

pub struct Config {
    pub wifi_iface: String,
    pub mon_iface: String,
    pub bt_iface: Option<String>,
    pub names_path: PathBuf,
    pub is_root: bool,
    pub cli: Option<CliMode>,
    pub demo: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let args: Vec<String> = env::args().collect();

        if args.iter().any(|a| a == "--help" || a == "-h") {
            print_help();
            std::process::exit(0);
        }
        if args.iter().any(|a| a == "--version" || a == "-V") {
            println!("airprowl {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        let cli = parse_cli(&args).unwrap_or_else(|e| {
            eprintln!("Error: {}\n", e);
            print_help();
            std::process::exit(2);
        });

        let wifi_iface = env::var("WIFI_IFACE").unwrap_or_else(|_| "wlo1".into());
        let mon_iface = format!("{}mon", wifi_iface);
        let bt_iface = env::var("BT_IFACE").ok();
        let names_path = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("names.json")))
            .unwrap_or_else(|| PathBuf::from("names.json"));
        let is_root = unsafe { libc::getuid() } == 0;
        let demo = args.iter().any(|a| a == "--demo");

        Self {
            wifi_iface,
            mon_iface,
            bt_iface,
            names_path,
            is_root,
            cli,
            demo,
        }
    }
}

fn parse_cli(args: &[String]) -> Result<Option<CliMode>, String> {
    let mut scan: Option<ScanTarget> = None;
    let mut duration_secs: u64 = DEFAULT_CLI_DURATION_SECS;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scan" => {
                let val = args.get(i + 1).ok_or("--scan requires a value (bt|wifi|subghz)")?;
                scan = Some(match val.as_str() {
                    "bt" | "bluetooth" => ScanTarget::Bt,
                    "wifi" | "wlan" => ScanTarget::Wifi,
                    "subghz" | "sub-ghz" | "sub_ghz" => ScanTarget::SubGhz,
                    other => return Err(format!("unknown scan target '{}'", other)),
                });
                i += 2;
            }
            "--duration" => {
                let val = args.get(i + 1).ok_or("--duration requires a value (seconds)")?;
                duration_secs = val.parse().map_err(|_| format!("--duration: '{}' is not a positive integer", val))?;
                i += 2;
            }
            "--help" | "-h" | "--version" | "-V" | "--demo" => {
                i += 1;
            }
            other => return Err(format!("unknown argument '{}'", other)),
        }
    }
    Ok(scan.map(|scan| CliMode { scan, duration_secs }))
}

fn print_help() {
    eprintln!("airprowl — real-time wireless scanner TUI for Linux");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  airprowl                              Launch interactive TUI (default)");
    eprintln!("  airprowl --demo                       Launch TUI with simulated devices");
    eprintln!("  airprowl --scan <TARGET> [OPTIONS]    Headless scan, JSON to stdout");
    eprintln!();
    eprintln!("CLI MODE:");
    eprintln!("  --scan <bt|wifi|subghz>   Scan target");
    eprintln!("  --duration <SECONDS>      How long to scan (default: 10)");
    eprintln!();
    eprintln!("GLOBAL OPTIONS:");
    eprintln!("  -h, --help                Show this help");
    eprintln!("  -V, --version             Show version");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("  WIFI_IFACE                WiFi interface (default: wlo1)");
    eprintln!("  BT_IFACE                  Bluetooth adapter (default: auto-detect)");
    eprintln!();
    eprintln!("EXAMPLES:");
    eprintln!("  airprowl                                 # Interactive TUI");
    eprintln!("  airprowl --scan bt --duration 5          # Quick 5s Bluetooth scan");
    eprintln!("  airprowl --scan wifi | jq '.devices[]'   # Pipe to jq");
    eprintln!("  sudo airprowl --scan wifi                # With monitor-mode client sniffing");
}
