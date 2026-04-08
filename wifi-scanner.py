#!/usr/bin/env python3
"""WiFi scanner that combines AP scan (nmcli) + client sniffing (monitor mode).

Streams JSON lines to stdout. Handles monitor mode setup/teardown.
"""

import json
import subprocess
import sys
import signal
import time
import os
import re
import threading

IFACE = os.environ.get("WIFI_IFACE", "wlo1")
MON_IFACE = IFACE + "mon"
IS_ROOT = os.geteuid() == 0
SUDO = [] if IS_ROOT else ["sudo", "-n"]

running = True

def handler(sig, frame):
    global running
    running = False

signal.signal(signal.SIGINT, handler)
signal.signal(signal.SIGTERM, handler)


def emit(data):
    print(json.dumps(data), flush=True)


def run(cmd, check=False):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=10)


def setup_monitor():
    """Put interface into monitor mode. Returns True on success."""
    # Check if already in monitor mode
    r = run(["iw", "dev"])
    if MON_IFACE in r.stdout:
        return True

    # Try creating a separate monitor interface first (keeps managed iface up)
    r = run([*SUDO, "iw", "dev", IFACE, "interface", "add", MON_IFACE, "type", "monitor"])
    if r.returncode == 0:
        run([*SUDO, "ip", "link", "set", MON_IFACE, "up"])
        return True

    # Fallback: switch main interface to monitor mode
    run([*SUDO, "ip", "link", "set", IFACE, "down"])
    r = run([*SUDO, "iw", "dev", IFACE, "set", "type", "monitor"])
    if r.returncode == 0:
        run([*SUDO, "ip", "link", "set", IFACE, "up"])
        return True

    # Restore managed mode on failure
    run([*SUDO, "iw", "dev", IFACE, "set", "type", "managed"])
    run([*SUDO, "ip", "link", "set", IFACE, "up"])
    return False


def teardown_monitor():
    """Restore managed mode."""
    # Try removing the dedicated monitor interface
    r = run([*SUDO, "ip", "link", "set", MON_IFACE, "down"])
    if r.returncode == 0:
        run([*SUDO, "iw", "dev", MON_IFACE, "del"])
        return

    # Otherwise restore the main interface
    run([*SUDO, "ip", "link", "set", IFACE, "down"])
    run([*SUDO, "iw", "dev", IFACE, "set", "type", "managed"])
    run([*SUDO, "ip", "link", "set", IFACE, "up"])


def scan_aps(skip_rescan=False):
    """Scan APs via nmcli and emit."""
    if not skip_rescan:
        subprocess.run(["nmcli", "dev", "wifi", "rescan"], capture_output=True, timeout=10)
        time.sleep(0.5)
    result = subprocess.run(
        ["nmcli", "-t", "-f", "SSID,BSSID,SIGNAL,FREQ,CHAN,SECURITY,MODE", "dev", "wifi", "list"],
        capture_output=True, text=True, timeout=10
    )
    for line in result.stdout.strip().split("\n"):
        if not line:
            continue
        parts = []
        current = ""
        i = 0
        while i < len(line):
            if line[i] == '\\' and i + 1 < len(line) and line[i + 1] == ':':
                current += ':'
                i += 2
            elif line[i] == ':':
                parts.append(current)
                current = ""
                i += 1
            else:
                current += line[i]
                i += 1
        parts.append(current)

        if len(parts) >= 6:
            try:
                sig = int(parts[2])
            except ValueError:
                continue
            emit({
                "type": "wifi_ap",
                "ssid": parts[0] or "(hidden)",
                "bssid": parts[1],
                "signal": sig,
                "freq": parts[3],
                "channel": parts[4],
                "security": parts[5],
            })


def ap_scanner_thread():
    """Periodically scan APs (works without monitor mode)."""
    first = True
    while running:
        try:
            scan_aps(skip_rescan=first)
            first = False
        except Exception:
            pass
        for _ in range(20):
            if not running:
                break
            time.sleep(0.5)


def channel_hopper(iface):
    """Hop across WiFi channels to catch more clients."""
    channels_2g = [1, 6, 11, 2, 3, 4, 5, 7, 8, 9, 10]
    channels_5g = [36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 149, 153, 157, 161, 165]
    channels = channels_2g + channels_5g
    idx = 0
    while running:
        try:
            ch = channels[idx % len(channels)]
            subprocess.run(
                [*SUDO, "iw", "dev", iface, "set", "channel", str(ch)],
                capture_output=True, timeout=5
            )
        except Exception:
            pass
        idx += 1
        time.sleep(0.3)


def sniff_clients(iface):
    """Use tshark to capture probe requests and data frames from WiFi clients."""
    # Capture probe requests (subtype 4) and data frames to see client MACs
    # -l for line-buffered, -T fields for parseable output
    proc = subprocess.Popen(
        [
            *SUDO, "tshark",
            "-i", iface,
            "-l",  # line buffered
            "-T", "fields",
            "-e", "wlan.sa",           # source MAC
            "-e", "wlan.da",           # destination MAC
            "-e", "radiotap.dbm_antsignal",  # RSSI
            "-e", "wlan_mgt.ssid",     # SSID from probe requests (renamed in newer tshark)
            "-e", "wlan.ssid",         # SSID field (alternate name)
            "-e", "wlan.fc.type_subtype",  # frame subtype
            "-E", "separator=|",
            "-E", "header=n",
            "-Y", "wlan.fc.type_subtype == 0x04 || wlan.fc.type_subtype == 0x05 || wlan.fc.type == 2",
            # probe req (4), probe resp (5), data frames (type 2)
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    known_aps = set()

    try:
        for line in proc.stdout:
            if not running:
                break
            line = line.strip()
            if not line:
                continue

            parts = line.split("|")
            if len(parts) < 6:
                continue

            src_mac = parts[0].strip()
            dst_mac = parts[1].strip()
            rssi_str = parts[2].strip()
            ssid = (parts[3] or parts[4] or "").strip()
            subtype_str = parts[5].strip()

            if not src_mac:
                continue

            # Parse RSSI (may have multiple antenna values comma-separated)
            rssi = None
            if rssi_str:
                try:
                    vals = [int(x) for x in rssi_str.split(",") if x.strip()]
                    rssi = max(vals) if vals else None  # use strongest antenna
                except ValueError:
                    pass

            # Determine if this is a probe request (client looking for networks)
            try:
                subtype = int(subtype_str, 0)
            except ValueError:
                subtype = -1

            is_probe_req = subtype == 4
            is_probe_resp = subtype == 5

            # Probe responses come from APs, track them to filter out AP MACs
            if is_probe_resp:
                known_aps.add(src_mac.lower())
                continue

            # Skip broadcast/multicast MACs
            if src_mac.lower() in ("ff:ff:ff:ff:ff:ff",):
                continue
            # Skip if src is a known AP
            if src_mac.lower() in known_aps:
                continue

            data = {
                "type": "wifi_client",
                "mac": src_mac,
            }
            if rssi is not None:
                data["rssi"] = rssi
            if ssid and is_probe_req:
                data["probing_for"] = ssid

            emit(data)

    except Exception:
        pass
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


def monitor_thread_fn():
    """Try to set up monitor mode and sniff clients (runs in background)."""
    mon_ok = setup_monitor()
    if not mon_ok:
        hint = "Run with sudo for client sniffing: sudo node app.js" if not IS_ROOT else "Monitor mode setup failed"
        emit({"type": "status", "msg": f"No monitor mode — {hint}. AP scanning only."})
        return

    sniff_iface = MON_IFACE
    r = run(["iw", "dev"])
    if MON_IFACE not in r.stdout:
        sniff_iface = IFACE

    hop_thread = threading.Thread(target=channel_hopper, args=(sniff_iface,), daemon=True)
    hop_thread.start()

    try:
        sniff_clients(sniff_iface)
    finally:
        teardown_monitor()


def main():
    # Start AP scanner immediately (no root needed)
    ap_thread = threading.Thread(target=ap_scanner_thread, daemon=True)
    ap_thread.start()

    # Try monitor mode in background so it doesn't block AP results
    mon_thread = threading.Thread(target=monitor_thread_fn, daemon=True)
    mon_thread.start()

    # Keep main thread alive
    while running:
        time.sleep(1)


if __name__ == "__main__":
    try:
        main()
    finally:
        teardown_monitor()
