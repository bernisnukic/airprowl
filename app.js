import React, { useState, useEffect, useCallback, useRef } from "react";
import { render, Box, Text, useInput, useApp, useStdout } from "ink";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const h = React.createElement;
const __dirname = dirname(fileURLToPath(import.meta.url));
const NAMES_FILE = join(__dirname, "names.json");

// ── Persistence ──

function loadNames() {
  try {
    return JSON.parse(readFileSync(NAMES_FILE, "utf-8"));
  } catch {
    return {};
  }
}

function saveNames(names) {
  writeFileSync(NAMES_FILE, JSON.stringify(names, null, 2) + "\n");
}

// ── Helpers ──

const EMA_ALPHA = 0.4;

function rssiToDistance(rssi, txPower) {
  const ref = txPower != null && txPower < 0 ? txPower : -59;
  const n = 2.5;
  return Math.pow(10, (ref - rssi) / (10 * n));
}

function wifiSignalToDbm(pct) {
  return -100 + (pct / 100) * 70;
}

function wifiDistanceEstimate(dbm) {
  const n = 3;
  const ref = -30;
  return Math.pow(10, (ref - dbm) / (10 * n));
}

function rssiColor(rssi) {
  if (rssi >= -60) return "green";
  if (rssi >= -75) return "yellow";
  if (rssi >= -85) return "#FFA500";
  return "red";
}

function signalPctColor(pct) {
  if (pct >= 70) return "green";
  if (pct >= 50) return "yellow";
  if (pct >= 30) return "#FFA500";
  return "red";
}

function distanceInfo(dist) {
  if (dist < 1) return { text: `${dist.toFixed(1)}m`, tag: "immediate", color: "green" };
  if (dist < 3) return { text: `${dist.toFixed(1)}m`, tag: "very close", color: "green" };
  if (dist < 8) return { text: `${dist.toFixed(1)}m`, tag: "nearby", color: "yellow" };
  if (dist < 15) return { text: `${dist.toFixed(1)}m`, tag: "same room", color: "#FFA500" };
  if (dist < 30) return { text: `${dist.toFixed(1)}m`, tag: "far", color: "red" };
  return { text: `${dist.toFixed(1)}m`, tag: "very far", color: "red" };
}

function trendArrow(ema, prevEma) {
  if (prevEma == null) return { symbol: " ~", color: "gray" };
  const diff = ema - prevEma;
  if (diff > 3) return { symbol: "↑↑", color: "green" };
  if (diff > 1) return { symbol: " ↑", color: "green" };
  if (diff < -3) return { symbol: "↓↓", color: "red" };
  if (diff < -1) return { symbol: " ↓", color: "red" };
  return { symbol: " ~", color: "gray" };
}

function formatAge(ms) {
  const s = Math.floor(ms / 1000);
  if (s < 1) return "<1s";
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m${s % 60}s`;
}

function pad(str, len) {
  return str.length >= len ? str.slice(0, len) : str + " ".repeat(len - str.length);
}

function padL(str, len) {
  return str.length >= len ? str.slice(0, len) : " ".repeat(len - str.length) + str;
}

// ── Shared Components ──

function SignalBar({ value, width = 20, maxVal = 100, minVal = 0 }) {
  const pct = (value - minVal) / (maxVal - minVal);
  const strength = Math.max(0, Math.min(width, Math.round(pct * width)));
  const color = pct >= 0.6 ? "green" : pct >= 0.4 ? "yellow" : pct >= 0.2 ? "#FFA500" : "red";
  return h(Text, null,
    h(Text, { color }, "█".repeat(strength)),
    h(Text, { dimColor: true }, "░".repeat(width - strength))
  );
}

function NamingBar({ id, editBuffer }) {
  return h(Box, { paddingX: 1, borderStyle: "single", borderColor: "magenta" },
    h(Text, { color: "magenta", bold: true }, "  Name device "),
    h(Text, { dimColor: true }, `[${id}]: `),
    h(Text, { color: "white", bold: true }, editBuffer),
    h(Text, { color: "magenta" }, "█"),
    h(Text, { dimColor: true }, "    [Enter] save  [Esc] cancel")
  );
}

// ── Bluetooth Components ──

function BtRow({ device, now, selected, customName }) {
  const { address, name, rssi, ema, prevEma, txPower, lastSeen, stale } = device;
  const dist = distanceInfo(rssiToDistance(ema, txPower));
  const trend = trendArrow(ema, prevEma);
  const age = formatAge(now - lastSeen);
  const color = rssiColor(rssi);
  const btName = name || address;

  let nameCol;
  if (customName) {
    nameCol = [
      h(Text, { key: "cn", color: "magenta", bold: true }, pad(customName, 16)),
      h(Text, { key: "bn", dimColor: true }, " " + pad(btName, 12)),
    ];
  } else {
    nameCol = [h(Text, { key: "bn", bold: !stale }, pad(btName, 29))];
  }

  const cursor = selected ? h(Text, { color: "cyan", bold: true }, "▸ ") : h(Text, null, "  ");

  if (stale) {
    return h(Box, null,
      cursor,
      h(Text, { dimColor: true },
        "? " + pad(customName || btName, 29) + "  " + pad(address, 18) + "  " +
        padL(`${rssi}`, 4) + " dBm  " + padL(`${Math.round(ema)}`, 4) + " dBm  " +
        "░".repeat(20) + "  " + padL(dist.text, 7) + " " +
        pad(`(${dist.tag})`, 12) + "  " + padL(age, 5)
      )
    );
  }

  return h(Box, null,
    cursor,
    h(Text, { color: trend.color }, trend.symbol + " "),
    ...nameCol,
    h(Text, { dimColor: true, color: "cyan" }, "  " + pad(address, 18)),
    h(Text, { color }, "  " + padL(`${rssi}`, 4) + " dBm"),
    h(Text, { color }, "  " + padL(`${Math.round(ema)}`, 4) + " dBm"),
    h(Text, null, "  "),
    h(SignalBar, { value: ema, minVal: -100, maxVal: -40 }),
    h(Text, { color: dist.color }, "  " + padL(dist.text, 7) + " "),
    h(Text, { dimColor: true, color: dist.color }, pad(`(${dist.tag})`, 12)),
    h(Text, { dimColor: true }, "  " + padL(age, 5))
  );
}

function BtColumnHeader() {
  return h(Box, null,
    h(Text, { dimColor: true, bold: true },
      "    " + pad("⬍", 3) + pad("Name", 29) + "  " + pad("MAC Address", 18) + "  " +
      padL("RSSI", 8) + "  " + padL("Avg", 8) + "  " + pad("Signal", 20) + "  " +
      pad("~Distance", 20) + "  " + padL("Seen", 5)
    )
  );
}

// ── WiFi Components ──

function WifiRow({ device, now, selected, customName }) {
  const { mac, kind, ssid, probingFor, signal, rssi, ema, prevEma, freq, channel, security, lastSeen, stale } = device;
  const isAp = kind === "ap";
  const age = formatAge(now - lastSeen);

  // Distance estimate
  let dist;
  if (isAp) {
    const dbm = wifiSignalToDbm(ema);
    dist = distanceInfo(wifiDistanceEstimate(dbm));
  } else {
    dist = ema != null ? distanceInfo(rssiToDistance(ema)) : { text: "?", tag: "", color: "gray" };
  }

  const trend = trendArrow(ema, prevEma);
  const cursor = selected ? h(Text, { color: "cyan", bold: true }, "▸ ") : h(Text, null, "  ");

  // Display name
  let displayName = isAp ? (ssid || "(hidden)") : mac;
  let nameCol;
  const kindTag = isAp
    ? h(Text, { key: "kt", color: "blue", dimColor: true }, "AP ")
    : h(Text, { key: "kt", color: "yellow" }, "📱 ");

  if (customName) {
    nameCol = [
      kindTag,
      h(Text, { key: "cn", color: "magenta", bold: true }, pad(customName, 14)),
      h(Text, { key: "dn", dimColor: true }, " " + pad(displayName, 9)),
    ];
  } else {
    nameCol = [
      kindTag,
      h(Text, { key: "dn", bold: !stale, color: displayName === "(hidden)" ? "gray" : undefined }, pad(displayName, 24)),
    ];
  }

  // Signal display
  let sigText, sigAvg, sigBar;
  if (isAp) {
    const color = signalPctColor(signal);
    sigText = h(Text, { color }, "  " + padL(`${signal}%`, 5));
    sigAvg = h(Text, { color }, "  " + padL(`${Math.round(ema)}%`, 5));
    sigBar = h(SignalBar, { value: ema, minVal: 0, maxVal: 100 });
  } else {
    const r = rssi || 0;
    const color = rssiColor(r);
    sigText = h(Text, { color }, "  " + padL(rssi != null ? `${rssi}` : "?", 4) + "dBm");
    sigAvg = h(Text, { color }, " " + padL(ema != null ? `${Math.round(ema)}` : "?", 4) + "dBm");
    sigBar = ema != null
      ? h(SignalBar, { value: ema, minVal: -100, maxVal: -40 })
      : h(Text, { dimColor: true }, "░".repeat(20));
  }

  // Extra info column: channel/freq for APs, probing SSID for clients
  let extraCol;
  if (isAp) {
    extraCol = h(Text, { dimColor: true }, "  " + pad((channel || "") + " " + (freq || ""), 13) + "  " + pad(security || "", 10));
  } else {
    const probe = probingFor ? `→ ${probingFor}` : "";
    extraCol = h(Text, { dimColor: true, color: "yellow" }, "  " + pad(probe, 25));
  }

  if (stale) {
    return h(Box, null,
      cursor,
      h(Text, { dimColor: true },
        "? " + (isAp ? "AP " : "📱 ") + pad(customName || displayName, 24) + "  " + pad(mac, 18) + "  " +
        padL(isAp ? `${signal}%` : (rssi != null ? `${rssi}dBm` : "?"), 7) + "  " +
        "░".repeat(20) + "  " + padL(dist.text, 7) + "  " + padL(age, 5)
      )
    );
  }

  return h(Box, null,
    cursor,
    h(Text, { color: trend.color }, trend.symbol + " "),
    ...nameCol,
    h(Text, { dimColor: true, color: "cyan" }, "  " + pad(mac, 18)),
    sigText,
    sigAvg,
    h(Text, null, "  "),
    sigBar,
    h(Text, { color: dist.color }, "  " + padL(dist.text, 7)),
    extraCol,
    h(Text, { dimColor: true }, "  " + padL(age, 5))
  );
}

function WifiColumnHeader() {
  return h(Box, null,
    h(Text, { dimColor: true, bold: true },
      "    " + pad("⬍", 3) + pad("Type / Name", 27) + "  " + pad("MAC / BSSID", 18) + "  " +
      padL("Signal", 8) + "  " + padL("Avg", 8) + "  " + pad("Strength", 20) + "  " +
      padL("~Dist", 7) + "  " + pad("Info", 25) + "  " + padL("Seen", 5)
    )
  );
}

// ── Header ──

function Header({ tab, elapsed, counts, sortMode, paused, editing }) {
  const mins = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const secs = String(elapsed % 60).padStart(2, "0");

  const btTab = tab === "bt"
    ? h(Text, { key: "bt", bold: true, color: "cyan", inverse: true }, " BT ")
    : h(Text, { key: "bt", dimColor: true }, " BT ");
  const wifiTab = tab === "wifi"
    ? h(Text, { key: "wifi", bold: true, color: "green", inverse: true }, " WiFi ")
    : h(Text, { key: "wifi", dimColor: true }, " WiFi ");

  return h(Box, {
    borderStyle: "round",
    borderColor: editing ? "magenta" : tab === "bt" ? "cyan" : "green",
    paddingX: 1,
    justifyContent: "space-between",
  },
    h(Text, null,
      h(Text, { bold: true, color: tab === "bt" ? "cyan" : "green" }, "  Scanner  "),
      btTab,
      h(Text, { dimColor: true }, " │ "),
      wifiTab,
    ),
    h(Text, null,
      h(Text, { dimColor: true }, " ⏱  "),
      h(Text, null, `${mins}:${secs}`),
      h(Text, { dimColor: true }, "  │  "),
      h(Text, { color: "green" }, `📡 ${counts.active}`),
      h(Text, { dimColor: true }, " active  "),
      h(Text, { dimColor: true }, `👻 ${counts.stale} stale`),
      h(Text, { dimColor: true }, "  │  "),
      h(Text, { dimColor: true }, `🔄 ${counts.updates}`),
      h(Text, { dimColor: true }, "  │  sort: "),
      h(Text, { color: "yellow" }, sortMode),
      paused ? h(Text, { color: "red" }, " ⏸ PAUSED") : null,
      h(Text, null, "  ")
    )
  );
}

function Footer({ editing, tab, wifiStatus }) {
  if (editing) return null;
  const tabHint = "[Tab] switch BT/WiFi  ";
  const common = "[q] quit  [↑↓] select  [n/Enter] name  [x] del name  [s] sort  [p] pause  [c] clear";
  return h(Box, { flexDirection: "column" },
    h(Box, { paddingX: 1 },
      h(Text, { dimColor: true }, tabHint + common)
    ),
    tab === "wifi" && wifiStatus
      ? h(Box, { paddingX: 1 },
          h(Text, { color: "yellow", dimColor: true }, "⚠ " + wifiStatus)
        )
      : null,
  );
}

// ── Main App ──

function App() {
  const { exit } = useApp();
  const { stdout } = useStdout();
  const rows = stdout?.rows || 24;

  const [tab, setTab] = useState("bt");
  const [btDevices, setBtDevices] = useState({});
  const [wifiDevices, setWifiDevices] = useState({});
  const [customNames, setCustomNames] = useState(loadNames);
  const [btUpdates, setBtUpdates] = useState(0);
  const [wifiUpdates, setWifiUpdates] = useState(0);
  const [startTime] = useState(Date.now());
  const [now, setNow] = useState(Date.now());
  const [sortMode, setSortMode] = useState("signal");
  const [paused, setPaused] = useState(false);
  const [wifiStatus, setWifiStatus] = useState(null);
  const [selectedId, setSelectedId] = useState(null);
  const [editing, setEditing] = useState(false);
  const [editBuffer, setEditBuffer] = useState("");
  const [frozenOrder, setFrozenOrder] = useState(null);

  const visibleListRef = useRef([]);

  // Tick
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(timer);
  }, []);

  // Bluetooth scanner
  useEffect(() => {
    const proc = spawn("python3", [join(__dirname, "scanner.py")], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    const rl = createInterface({ input: proc.stdout });

    rl.on("line", (line) => {
      try {
        const data = JSON.parse(line);
        const addr = data.address;
        if (!addr) return;

        setBtDevices((prev) => {
          const existing = prev[addr] || {
            address: addr, ema: null, prevEma: null, sampleCount: 0, firstSeen: Date.now(),
          };
          const updated = { ...existing, lastSeen: Date.now() };
          if (data.Name) updated.name = data.Name;
          if (data.Alias && !updated.name) updated.name = data.Alias;
          if (data.RSSI != null) {
            updated.rssi = data.RSSI;
            updated.sampleCount = existing.sampleCount + 1;
            if (existing.ema == null) {
              updated.ema = data.RSSI;
              updated.prevEma = null;
            } else {
              if (updated.sampleCount % 5 === 0) updated.prevEma = existing.ema;
              else updated.prevEma = existing.prevEma;
              updated.ema = EMA_ALPHA * data.RSSI + (1 - EMA_ALPHA) * existing.ema;
            }
          }
          if (data.TxPower != null) updated.txPower = data.TxPower;
          return { ...prev, [addr]: updated };
        });
        setBtUpdates((c) => c + 1);
      } catch { /* ignore */ }
    });
    proc.stderr.on("data", () => {});
    return () => { proc.kill("SIGTERM"); rl.close(); };
  }, []);

  // WiFi scanner (APs + clients)
  useEffect(() => {
    const proc = spawn("python3", [join(__dirname, "wifi-scanner.py")], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    const rl = createInterface({ input: proc.stdout });

    rl.on("line", (line) => {
      try {
        const data = JSON.parse(line);

        if (data.type === "status") {
          setWifiStatus(data.msg);
          return;
        }

        if (data.type === "wifi_ap" && data.bssid) {
          const id = data.bssid;
          setWifiDevices((prev) => {
            const existing = prev[id] || {
              mac: id, kind: "ap", ema: null, prevEma: null, sampleCount: 0, firstSeen: Date.now(),
            };
            const updated = { ...existing, lastSeen: Date.now(), kind: "ap" };
            updated.ssid = data.ssid;
            updated.freq = data.freq;
            updated.channel = data.channel;
            updated.security = data.security;

            if (data.signal != null) {
              updated.signal = data.signal;
              updated.sampleCount = existing.sampleCount + 1;
              if (existing.ema == null) {
                updated.ema = data.signal;
                updated.prevEma = null;
              } else {
                if (updated.sampleCount % 3 === 0) updated.prevEma = existing.ema;
                else updated.prevEma = existing.prevEma;
                updated.ema = EMA_ALPHA * data.signal + (1 - EMA_ALPHA) * existing.ema;
              }
            }
            return { ...prev, [id]: updated };
          });
          setWifiUpdates((c) => c + 1);

        } else if (data.type === "wifi_client" && data.mac) {
          const id = data.mac;
          setWifiDevices((prev) => {
            const existing = prev[id] || {
              mac: id, kind: "client", ema: null, prevEma: null, sampleCount: 0, firstSeen: Date.now(),
            };
            const updated = { ...existing, lastSeen: Date.now(), kind: "client" };
            if (data.probing_for) updated.probingFor = data.probing_for;

            if (data.rssi != null) {
              updated.rssi = data.rssi;
              updated.sampleCount = existing.sampleCount + 1;
              if (existing.ema == null) {
                updated.ema = data.rssi;
                updated.prevEma = null;
              } else {
                if (updated.sampleCount % 5 === 0) updated.prevEma = existing.ema;
                else updated.prevEma = existing.prevEma;
                updated.ema = EMA_ALPHA * data.rssi + (1 - EMA_ALPHA) * existing.ema;
              }
            }
            return { ...prev, [id]: updated };
          });
          setWifiUpdates((c) => c + 1);
        }
      } catch { /* ignore */ }
    });
    proc.stderr.on("data", () => {});
    return () => { proc.kill("SIGTERM"); rl.close(); };
  }, []);

  // Keyboard
  const inputHandler = useCallback(
    (input, key) => {
      const list = visibleListRef.current;

      if (editing) {
        if (key.escape) {
          setEditing(false);
          setEditBuffer("");
          setFrozenOrder(null);
          return;
        }
        if (key.return) {
          if (selectedId) {
            setCustomNames((prev) => {
              const next = { ...prev };
              const trimmed = editBuffer.trim();
              if (trimmed) next[selectedId] = trimmed;
              else delete next[selectedId];
              saveNames(next);
              return next;
            });
          }
          setEditing(false);
          setEditBuffer("");
          setFrozenOrder(null);
          return;
        }
        if (key.backspace || key.delete) {
          setEditBuffer((b) => b.slice(0, -1));
          return;
        }
        if (input && !key.ctrl && !key.meta && !key.escape) {
          setEditBuffer((b) => b + input);
        }
        return;
      }

      // Normal mode
      if (input === "q" || (input === "c" && key.ctrl)) exit();

      if (key.tab) {
        setTab((t) => t === "bt" ? "wifi" : "bt");
        setSelectedId(null);
        setSortMode("signal");
      }

      if (key.upArrow) {
        const idx = list.findIndex((d) => d._id === selectedId);
        const newIdx = Math.max(0, (idx < 0 ? 0 : idx) - 1);
        if (list[newIdx]) setSelectedId(list[newIdx]._id);
      }
      if (key.downArrow) {
        const idx = list.findIndex((d) => d._id === selectedId);
        const newIdx = Math.min(list.length - 1, (idx < 0 ? 0 : idx) + 1);
        if (list[newIdx]) setSelectedId(list[newIdx]._id);
      }

      if (input === "n" || key.return) {
        if (selectedId) {
          setEditing(true);
          setEditBuffer(customNames[selectedId] || "");
          setFrozenOrder(list.map((d) => d._id));
        }
      }

      if (input === "x") {
        if (selectedId) {
          setCustomNames((prev) => {
            const next = { ...prev };
            delete next[selectedId];
            saveNames(next);
            return next;
          });
        }
      }

      if (input === "s") {
        setSortMode((m) => {
          if (tab === "bt") return m === "signal" ? "distance" : m === "distance" ? "name" : "signal";
          return m === "signal" ? "name" : m === "name" ? "type" : "signal";
        });
      }
      if (input === "p") setPaused((p) => !p);
      if (input === "c" && !key.ctrl) {
        if (tab === "bt") { setBtDevices({}); setBtUpdates(0); }
        else { setWifiDevices({}); setWifiUpdates(0); }
        setSelectedId(null);
      }
    },
    [exit, editing, editBuffer, selectedId, customNames, tab]
  );
  useInput(inputHandler, { isActive: process.stdin.isTTY === true });

  // ── Build visible list ──
  const elapsed = Math.floor((now - startTime) / 1000);
  const maxRows = Math.max(5, rows - (editing ? 9 : 7));
  let visibleList = [];
  let activeCount = 0;
  let staleCount = 0;
  let updateCount = tab === "bt" ? btUpdates : wifiUpdates;

  if (tab === "bt") {
    const active = [];
    const stale = [];
    for (const d of Object.values(btDevices)) {
      if (d.rssi == null) continue;
      const age = now - d.lastSeen;
      if (age > 120000) continue;
      const entry = { ...d, _id: d.address, stale: age > 15000 };
      (entry.stale ? stale : active).push(entry);
    }

    const sortFn =
      sortMode === "distance"
        ? (a, b) => rssiToDistance(a.ema, a.txPower) - rssiToDistance(b.ema, b.txPower)
        : sortMode === "name"
          ? (a, b) => (customNames[a.address] || a.name || a.address).localeCompare(customNames[b.address] || b.name || b.address)
          : (a, b) => b.rssi - a.rssi;

    active.sort(sortFn);
    stale.sort(sortFn);
    activeCount = active.length;
    staleCount = stale.length;
    visibleList = [...active, ...stale].slice(0, maxRows);
  } else {
    const active = [];
    const stale = [];
    for (const d of Object.values(wifiDevices)) {
      const hasSig = d.kind === "ap" ? d.signal != null : d.rssi != null;
      if (!hasSig && d.ema == null) continue;
      const age = now - d.lastSeen;
      if (age > 120000) continue;
      const staleThresh = d.kind === "ap" ? 30000 : 15000;
      const entry = { ...d, _id: d.mac, stale: age > staleThresh };
      (entry.stale ? stale : active).push(entry);
    }

    const getSignal = (d) => {
      if (d.kind === "ap") return d.ema || 0;
      // Normalize client RSSI to comparable scale (map -100...-40 to 0...100)
      return d.ema != null ? Math.max(0, Math.min(100, ((d.ema + 100) / 60) * 100)) : 0;
    };

    const sortFn =
      sortMode === "name"
        ? (a, b) => (customNames[a.mac] || a.ssid || a.probingFor || a.mac).localeCompare(customNames[b.mac] || b.ssid || b.probingFor || b.mac)
        : sortMode === "type"
          ? (a, b) => {
              if (a.kind !== b.kind) return a.kind === "client" ? -1 : 1;
              return getSignal(b) - getSignal(a);
            }
          : (a, b) => getSignal(b) - getSignal(a);

    active.sort(sortFn);
    stale.sort(sortFn);
    activeCount = active.length;
    staleCount = stale.length;
    visibleList = [...active, ...stale].slice(0, maxRows);
  }

  // Freeze order while editing
  if (editing && frozenOrder) {
    const byId = Object.fromEntries(visibleList.map((d) => [d._id, d]));
    const frozen = frozenOrder.filter((id) => byId[id]).map((id) => byId[id]);
    const frozenSet = new Set(frozenOrder);
    for (const d of visibleList) {
      if (!frozenSet.has(d._id)) frozen.push(d);
    }
    visibleList = frozen.slice(0, maxRows);
  }

  visibleListRef.current = visibleList;

  if (!selectedId && visibleList.length > 0) {
    setSelectedId(visibleList[0]._id);
  }

  const selectedIdx = visibleList.findIndex((d) => d._id === selectedId);

  // ── Render ──
  const colHeader = tab === "bt" ? h(BtColumnHeader) : h(WifiColumnHeader);

  const deviceRows = visibleList.length === 0
    ? h(Box, { justifyContent: "center", paddingY: 1 },
        h(Text, { dimColor: true }, tab === "wifi" ? "Scanning... (monitor mode may need sudo)" : "Scanning for devices...")
      )
    : visibleList.map((d, i) => {
        const sel = i === selectedIdx;
        const cn = customNames[d._id] || null;
        if (tab === "bt") {
          return h(BtRow, { key: d._id, device: d, now, selected: sel, customName: cn });
        } else {
          return h(WifiRow, { key: d._id, device: d, now, selected: sel, customName: cn });
        }
      });

  return h(Box, { flexDirection: "column", width: "100%" },
    h(Header, {
      tab, elapsed, counts: { active: activeCount, stale: staleCount, updates: updateCount },
      sortMode, paused, editing,
    }),
    colHeader,
    h(Box, { flexDirection: "column", flexGrow: 1 }, deviceRows),
    editing && selectedId
      ? h(NamingBar, { id: selectedId, editBuffer })
      : null,
    h(Footer, { editing, tab, wifiStatus })
  );
}

// Enter alternate screen buffer (fullscreen TUI mode)
function enterFullscreen() {
  if (!process.stdin.isTTY) return;
  process.stdout.write("\x1b[?1049h");
  process.stdout.write("\x1b[H");
  process.stdout.write("\x1b[2J");
  process.stdout.write("\x1b[?25l");
}

function exitFullscreen() {
  if (!process.stdin.isTTY) return;
  process.stdout.write("\x1b[?25h");
  process.stdout.write("\x1b[?1049l");
}

enterFullscreen();

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => { exitFullscreen(); process.exit(0); });
}
process.on("exit", exitFullscreen);

const instance = render(h(App), { exitOnCtrlC: true });
instance.waitUntilExit().then(() => { exitFullscreen(); process.exit(0); });
