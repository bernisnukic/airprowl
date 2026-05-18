# airprowl

Real-time wireless scanner for Linux. Tracks nearby Bluetooth, WiFi (APs + clients), and Sub-GHz (315/433/868/915 MHz) devices with live RSSI, EMA-smoothed averages, trend arrows, distance estimates, vendor identification, and persistent custom names. Runs as an interactive TUI or as a headless CLI that emits JSON.

![Rust](https://img.shields.io/badge/rust-edition_2021-orange)
![Platform](https://img.shields.io/badge/platform-Linux-blue)
![License](https://img.shields.io/badge/license-MIT-yellow)

---

## Table of Contents

1. [Features](#features)
2. [Prerequisites](#prerequisites)
3. [Build](#build)
4. [Interactive TUI](#interactive-tui)
5. [CLI mode (headless JSON)](#cli-mode-headless-json)
6. [Exported JSON schema](#exported-json-schema)
7. [How it works](#how-it-works)
8. [Project structure](#project-structure)
9. [Configuration](#configuration)
10. [Bundled data](#bundled-data)
11. [License](#license)

---

## Features

- **Tri-band scanning** — Bluetooth, WiFi, and Sub-GHz ISM bands, one tool
- **Bluetooth (BR/EDR + BLE)** — direct BlueZ D-Bus, queries `address_type`, `manufacturer_data`, `icon`
- **WiFi APs** — periodic `nmcli` scans (no root needed)
- **WiFi clients** — virtual monitor interface + 802.11 frame parsing (probe requests, data, QoS) for passive sniffing
- **Sub-GHz** — direct USB control of a Yard Stick One (CC1111) across 315/433/868/915 MHz
- **Vendor identification** —
  - Embedded IEEE OUI database (49K entries from `nmap-mac-prefixes`)
  - Embedded Bluetooth SIG Company ID database (3971 entries)
  - BlueZ-inferred device class (`phone`, `audio-card`, `watch`, `computer`, etc.)
  - Combined output like `(Apple, phone)`, `(Bose, audio-headset)`, `(Samsung, watch)`
  - Detects BLE Resolvable Private Addresses → `(random)`
- **Live signal tracking** — RSSI smoothed with EMA, periodic snapshots feed trend arrows (↑↑ ↑ ~ ↓ ↓↓)
- **Distance estimation** — log-distance path-loss model
- **Sortable views** — by signal, distance, name, or kind
- **Custom names** — name any device; persisted to `names.json`
- **Two-tier liveness** — devices go stale at 15–30 s (dimmed), dropped at 120 s
- **WiFi connect** — `w` key triggers `nmcli` connect, with password modal for secured APs
- **Export** — `e` key dumps current tab to JSON in the working directory
- **Interface health surfaced in the UI** — down WiFi iface or stopped bluetoothd shows up in the footer with the fix

---

## Prerequisites

```bash
# Debian/Ubuntu
sudo apt install network-manager iw bluez libusb-1.0-0 libdbus-1-dev pkg-config

# Arch
sudo pacman -S networkmanager iw bluez libusb pkg-config
```

Rust **1.75+** (edition 2021).

**Optional hardware:** [Yard Stick One](https://greatscottgadgets.com/yardstickone/) (or any CC1111 dongle with USB VID `0x1d50` and PID in `0x6047/0x6048/0x604f/0x605b`) for Sub-GHz. Without it, the Sub-GHz tab stays empty — the rest works.

**Required permissions:**
| Capability | Needs root? |
|---|---|
| WiFi AP scanning (nmcli) | no |
| Bluetooth (BlueZ D-Bus) | no |
| WiFi client sniffing (monitor mode) | **yes** |
| Sub-GHz / Yard Stick USB | **yes** |

Run without sudo for BT + AP scanning. Run with `sudo` for the full feature set.

---

## Build

```bash
git clone https://github.com/bernisnukic/airprowl.git
cd airprowl
cargo build --release
```

Outputs `target/release/airprowl` (~4 MB, self-contained — OUI + Bluetooth SIG databases are embedded).

---

## Interactive TUI

```bash
./target/release/airprowl              # BT + WiFi APs (no root)
sudo ./target/release/airprowl         # adds client sniffing + Yard Stick
```

### Keyboard shortcuts

| Key            | Action                                                       |
|----------------|--------------------------------------------------------------|
| `Tab`          | Cycle tabs: **BT → WiFi → Sub-GHz**                          |
| `↑` / `↓`      | Select device                                                |
| `n` / `Enter`  | Name the selected device (persists to `names.json`)          |
| `Esc`          | Cancel rename / password input                               |
| `x`            | Delete custom name                                           |
| `s`            | Cycle sort mode (signal / distance / name / type)            |
| `p`            | Pause / resume updates                                       |
| `c`            | Clear current tab's device list                              |
| `e`            | Export current tab to `airprowl-<tab>-<timestamp>.json`      |
| `w` *(WiFi)*   | Connect to selected AP via `nmcli` (password modal if secured)|
| `q` / `Ctrl-C` | Quit                                                         |

### Display conventions

| Style                 | Meaning                                                |
|-----------------------|--------------------------------------------------------|
| **Bold white** name   | Real device-advertised name                            |
| *Dim gray* `(vendor)` | OUI / Bluetooth SIG fallback (no advertised name)      |
| *Dim gray* `(random)` | BLE Resolvable Private Address or LAA-flagged WiFi MAC |
| Magenta name          | User-set custom name (via `n`)                         |
| Dimmed full row       | Stale (no updates in 15–30 s)                          |
| Color-coded RSSI      | Green ≥ -60 dBm, Yellow ≥ -75, Orange ≥ -85, Red < -85 |
| Trend arrows          | ↑↑ rising fast · ↑ rising · ~ stable · ↓ falling · ↓↓ falling fast |

### Status messages

The footer surfaces actionable diagnostics:
- *"WiFi iface wlo1 is DOWN — run: sudo ip link set wlo1 up"*
- *"WiFi iface wlan0 not found — set WIFI_IFACE=<name> (see: ip -br link)"*
- *"No monitor mode — … AP scanning only."*
- *"Yard Stick One not found"*
- *"\<bluer error\> — try: sudo systemctl start bluetooth"*

Once you fix the underlying issue (e.g. bring the WiFi iface up), the warning clears on the next scan cycle.

---

## CLI mode (headless JSON)

For scripting, monitoring, or piping into `jq`. Spawns scanners, waits `--duration` seconds, then prints a JSON document to stdout and exits.

```bash
airprowl --scan bt                         # 10s BT scan (default duration)
airprowl --scan wifi --duration 5          # 5s WiFi scan
airprowl --scan subghz --duration 30       # 30s Sub-GHz sweep
sudo airprowl --scan wifi --duration 15    # WiFi w/ monitor-mode client sniffing

# Pipe into jq for ad-hoc analysis
airprowl --scan bt | jq '.devices[] | select(.vendor == "Apple")'
airprowl --scan wifi | jq '.devices[] | select(.kind == "ap") | {ssid, security, signal_pct}'

# Count devices by vendor
airprowl --scan bt | jq -r '.devices[].vendor // "unknown"' | sort | uniq -c | sort -rn
```

### Output shape (top level)

```json
{
  "scan_type": "bt",
  "duration_secs": 10,
  "device_count": 47,
  "status": ["WiFi iface wlo1 is DOWN — run: sudo ip link set wlo1 up"],
  "devices": [ /* see schema below */ ]
}
```

The `status` array captures any warnings the scanners emitted during the run (down interfaces, missing devices, etc.).

---

## Exported JSON schema

Both the TUI `e` key and CLI mode produce the same per-device shapes.

### Bluetooth (`scan_type: "bt"`)

```json
{
  "address": "B0:60:88:49:C5:CD",
  "name": "Bedroom speaker",          // BlueZ-advertised name, null if none
  "vendor": "Sony, audio-card",       // OUI/BLE-SIG vendor + BlueZ icon, "random" for RPA
  "icon": "audio-card",               // BlueZ icon hint (phone, watch, etc.)
  "company_id": 76,                   // Bluetooth SIG Company ID (0x4C = Apple)
  "is_random": false,                 // true = BlueZ reports LeRandom address type
  "rssi_dbm": -65,
  "rssi_avg_dbm": -67.2,
  "tx_power": -59,
  "distance_m": 3.5,
  "samples": 124,
  "seen_secs_ago": 0,
  "tracked_secs": 28,
  "custom_name": null                 // user-set name via `n` key
}
```

### WiFi (`scan_type: "wifi"`)

```json
{
  "mac": "AA:BB:CC:DD:EE:FF",
  "kind": "ap",                       // "ap" or "client"
  "ssid": "HomeNetwork",              // null for clients without probe target
  "vendor": "Cisco",                  // OUI-derived; "random" for LAA MACs
  "probing_for": null,                // for clients: SSID they sent probe-request for
  "signal_pct": 78.0,                 // APs only (nmcli percentage)
  "signal_pct_avg": 76.3,
  "rssi_dbm": -52,                    // clients only (radiotap-extracted)
  "rssi_avg_dbm": -53.1,
  "channel": "44",
  "frequency": "5220 MHz",
  "security": "WPA2",
  "distance_m": 2.8,
  "samples": 12,
  "seen_secs_ago": 1,
  "tracked_secs": 35,
  "custom_name": null
}
```

### Sub-GHz (`scan_type: "subghz"`)

```json
{
  "freq_hz": 433920000,
  "freq_mhz": 433.92,
  "band": "433 MHz",
  "rssi_dbm": -62,
  "rssi_avg_dbm": -64.1,
  "samples": 8,
  "seen_secs_ago": 2,
  "tracked_secs": 14,
  "custom_name": null
}
```

---

## How it works

`main.rs` parses CLI args, then either runs `cli::run()` (headless) or spawns scanner tasks that fan into a single `tokio::sync::mpsc` channel and launches the `App` event loop in `src/app.rs`. The TUI is built with [ratatui](https://github.com/ratatui/ratatui).

| Scanner | File | Mechanism |
|---|---|---|
| Bluetooth | `scanner/bluetooth.rs` | `bluer` D-Bus session, `discover_devices()` + per-device property streams, queries `address_type()` / `icon()` / `manufacturer_data()` |
| WiFi AP | `scanner/wifi/ap_scanner.rs` | Periodic `nmcli -t dev wifi list`, escaped-colon parser, `/sys/class/net/.../operstate` health check |
| WiFi monitor | `scanner/wifi/monitor.rs` | `iw dev … interface add … type monitor` creates virtual iface; `Drop` impl tears it down |
| WiFi clients | `scanner/wifi/client_sniffer.rs` | `pnet` raw datalink, `libwifi` 802.11 parser, hand-rolled radiotap RSSI extractor |
| Sub-GHz | `scanner/subghz/yardstick.rs` | Direct USB bulk transfers to CC1111: pokes FREQ2/FREQ1/FREQ0 registers, strobes SCAL/SRX, reads RSSI |

**Signal smoothing.** RSSI is smoothed with an EMA (`α = 0.4`). Every N samples (per-source constants in `config.rs`) a previous-EMA snapshot is captured so trend arrows reflect meaningful deltas, not per-sample jitter. Distance is the log-distance path-loss model with per-source reference RSSI and exponent.

**Vendor identification.** For each MAC we try, in order:
1. **OUI lookup** (for non-random addresses) against `data/oui-prefixes.txt`
2. **Bluetooth SIG Company ID** (for BT devices with manufacturer data) against `data/ble-companies.txt`
3. **BlueZ icon** hint as a supplementary device-class label
4. **"random"** fallback for BLE Resolvable Private Addresses (BlueZ reports `LeRandom`) or WiFi MACs with the LAA bit set

The result is shortened to fit the table column ("Samsung Electronics Co. Ltd." → "Samsung", "Apple, Inc." → "Apple").

**Shutdown.** Quitting the TUI closes the event channel; scanners detect this via `tx.closed()` / `tx.is_closed()` and exit, allowing their `Drop` impls (USB reset, monitor-iface teardown) to run before the runtime drops.

---

## Project structure

```
airprowl/
  data/
    oui-prefixes.txt        IEEE OUI database (sourced from nmap-mac-prefixes,
                            embedded into the binary via include_str!)
    ble-companies.txt       Bluetooth SIG Company Identifiers
                            (sourced from bluetooth.com, embedded)
  src/
    main.rs                 Entry point, scanner task fan-in, CLI/TUI dispatch
    cli.rs                  Headless --scan mode runner
    app.rs                  TUI event loop, per-tab rendering, key handling
    config.rs               Compile-time constants + runtime arg parsing
    events.rs               AppEvent enum (BT/WiFi/Sub-GHz/Tick/Key)
    store.rs                Device structs + EMA update functions
    signal.rs               EMA, distance, color, trend-arrow helpers
    names.rs                Custom-name persistence (atomic JSON write)
    oui.rs                  IEEE OUI lookup + combined BT vendor label
    ble.rs                  Bluetooth SIG Company ID lookup
    export.rs               Serializable export structs (JSON)
    util.rs                 unicode-width-aware string padding
    scanner/
      bluetooth.rs          BlueZ D-Bus
      wifi/
        ap_scanner.rs       nmcli polling + iface health check
        monitor.rs          iw monitor-iface lifecycle
        client_sniffer.rs   pnet + libwifi + radiotap
      subghz/
        yardstick.rs        CC1111 USB control
    tui/
      mod.rs                Terminal setup / restore
      state.rs              TuiState (tab, sort, selection, scroll, modals)
      widgets/
        header.rs
        footer.rs
        bt_table.rs
        wifi_table.rs
        subghz_table.rs
```

---

## Configuration

### Environment variables

| Variable     | Default        | Description                                  |
|--------------|----------------|----------------------------------------------|
| `WIFI_IFACE` | `wlo1`         | WiFi interface (managed mode)                |
| `BT_IFACE`   | auto-detect    | Bluetooth adapter (e.g. `hci0`)              |

The monitor interface name is derived as `${WIFI_IFACE}mon` (e.g. `wlo1mon`).

### Tuning constants

All in `src/config.rs`:

| Constant                   | Default | Meaning                                              |
|----------------------------|---------|------------------------------------------------------|
| `EMA_ALPHA`                | `0.4`   | EMA weight on new sample                             |
| `BT_STALE_MS`              | `15000` | BT device considered stale (dimmed)                  |
| `AP_STALE_MS`              | `30000` | WiFi AP considered stale                             |
| `CLIENT_STALE_MS`          | `15000` | WiFi client considered stale                         |
| `DROP_MS`                  | `120000`| Device dropped from any list                         |
| `AP_SCAN_INTERVAL_MS`      | `10000` | Time between nmcli AP rescans                        |
| `SUBGHZ_STEP_DELAY_US`     | `500`   | Inter-frequency delay during Sub-GHz sweep           |
| `SHUTDOWN_GRACE_MS`        | `200`   | Pause after quit for USB/monitor-iface cleanup       |
| `DEFAULT_CLI_DURATION_SECS`| `10`    | `--duration` default when omitted                    |

### Custom device names

Pressing `n` (or `Enter`) on a selected device opens a name modal. The name persists to `names.json` next to the binary, keyed by:
- BT: MAC address (e.g. `B0:60:88:49:C5:CD`)
- WiFi: MAC address (works for both APs and clients)
- Sub-GHz: frequency in Hz as decimal string (e.g. `433920000`)

Custom names render in magenta and take display priority over device-advertised names and vendor fallbacks. They also appear in JSON exports under `custom_name`.

---

## Bundled data

`data/oui-prefixes.txt` — sourced from nmap-mac-prefixes (`/usr/share/nmap/nmap-mac-prefixes` on most distros). Underlying data is the public IEEE OUI registry. Refresh by replacing the file and rebuilding.

`data/ble-companies.txt` — sourced from the [Bluetooth SIG public assigned-numbers repository](https://bitbucket.org/bluetooth-SIG/public/src/main/assigned_numbers/company_identifiers/company_identifiers.yaml). Converted to a flat `<4-hex-id> <name>` format. Refresh similarly.

Both are embedded into the binary via `include_str!` at compile time — no runtime file or network access required.

---

## License

MIT — see `LICENSE`.
