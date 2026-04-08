# airprowl

Real-time Bluetooth & WiFi scanner TUI for Linux. Tracks nearby wireless devices with live signal strength, distance estimation, trend indicators, and custom device naming.

![Node.js](https://img.shields.io/badge/node-%3E%3D18-green)
![Platform](https://img.shields.io/badge/platform-Linux-blue)
![License](https://img.shields.io/badge/license-MIT-yellow)

## Features

- **Dual-mode scanning** — Bluetooth (BlueZ/D-Bus) and WiFi (nmcli + tshark) in one TUI
- **Live signal tracking** — RSSI with exponential moving average smoothing
- **Distance estimation** — approximate range based on signal strength
- **Trend indicators** — arrows showing signal direction over time
- **Device naming** — assign custom names to devices, persisted to disk
- **WiFi client sniffing** — monitor mode captures probe requests and data frames
- **Channel hopping** — cycles 2.4GHz + 5GHz channels for broader coverage
- **Sortable views** — sort by signal, distance, name, or device type

## Prerequisites

**System dependencies:**

```bash
# Debian/Ubuntu
sudo apt install network-manager tshark iw bluez python3 python3-dbus python3-gi

# Arch
sudo pacman -S networkmanager wireshark-cli iw bluez python python-dbus python-gobject
```

**Node.js** >= 18

## Install

```bash
# Clone and run
git clone https://github.com/bernisnukic/airprowl.git
cd airprowl
npm install
npm start

# Or install globally
npm install -g .
airprowl
```

## Usage

```bash
# AP scanning only (no root needed)
node src/app.js

# Full mode: AP + client sniffing + channel hopping
sudo node src/app.js

# Custom interfaces
WIFI_IFACE=wlan0 BT_IFACE=hci1 node src/app.js
```

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Tab` | Switch between BT / WiFi tabs |
| `Up/Down` | Select device |
| `n` / `Enter` | Name selected device |
| `x` | Delete custom name |
| `s` | Cycle sort mode |
| `p` | Pause/resume updates |
| `c` | Clear device list |
| `q` | Quit |

## Build standalone binary

```bash
npm run build
```

Outputs to `dist/`:
- `airprowl` — standalone Node.js binary
- `src/bt-scanner.py` — Bluetooth scanner backend
- `src/wifi-scanner.py` — WiFi scanner backend

Ship the entire `dist/` folder. Requires Python 3 + system deps on the target machine.

## Project structure

```
airprowl/
  bin/airprowl.js        CLI entry point
  src/
    app.js               TUI app (Ink/React)
    bt-scanner.py        Bluetooth scanner (BlueZ D-Bus)
    wifi-scanner.py      WiFi scanner (nmcli + tshark)
  scripts/
    build.js             Binary build script
```

## How it works

The TUI (`src/app.js`) is built with [Ink](https://github.com/vadimdemedes/ink) (React for terminals). It spawns two Python backend processes that stream JSON lines to stdout:

- **bt-scanner.py** — listens to BlueZ D-Bus signals for Bluetooth device discovery and RSSI updates
- **wifi-scanner.py** — runs `nmcli` for AP scanning and `tshark` in monitor mode for client sniffing, with channel hopping across 2.4GHz and 5GHz bands

Signal readings are smoothed with an exponential moving average (EMA) and converted to approximate distances using the log-distance path loss model.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `WIFI_IFACE` | `wlo1` | WiFi interface for scanning |
| `BT_IFACE` | auto-detect | Bluetooth adapter (e.g. `hci0`) |

## License

MIT
