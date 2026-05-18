use std::collections::HashMap;
use std::io::Stdout;
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout};
use tokio::sync::mpsc;

use crate::ble::BleCompanyDb;
use crate::config::{self, Config};
use crate::events::AppEvent;
use crate::export;
use crate::names::NameStore;
use crate::oui::{self, OuiDb};
use crate::signal;
use crate::store::{self, BtDevice, SubGhzDevice, WifiDevice, WifiKind};
use crate::tui::state::{Tab, SortMode, TuiState};
use crate::tui::widgets::{header, footer, bt_table, wifi_table, subghz_table};

pub struct App {
    names: NameStore,
    oui_db: Option<OuiDb>,
    ble_db: Option<BleCompanyDb>,
    bt_devices: HashMap<String, BtDevice>,
    wifi_devices: HashMap<String, WifiDevice>,
    subghz_devices: HashMap<String, SubGhzDevice>,
    bt_updates: u64,
    wifi_updates: u64,
    subghz_updates: u64,
    start_time: Instant,
    tui: TuiState,
    event_rx: mpsc::Receiver<AppEvent>,
    event_tx: mpsc::Sender<AppEvent>,
}

/// Helper: get the ID string for an entry in either list type
fn bt_id(list: &[bt_table::BtListEntry], idx: usize) -> Option<String> {
    list.get(idx).map(|e| e.device.address.clone())
}

fn wifi_id(list: &[wifi_table::WifiListEntry], idx: usize) -> Option<String> {
    list.get(idx).map(|e| e.device.mac.clone())
}

fn find_bt_index(list: &[bt_table::BtListEntry], id: &str) -> Option<usize> {
    list.iter().position(|e| e.device.address == id)
}

fn find_wifi_index(list: &[wifi_table::WifiListEntry], id: &str) -> Option<usize> {
    list.iter().position(|e| e.device.mac == id)
}

impl App {
    pub fn new(
        config: Arc<Config>,
        event_tx: mpsc::Sender<AppEvent>,
        event_rx: mpsc::Receiver<AppEvent>,
    ) -> Self {
        let names = NameStore::load(&config.names_path);
        let oui_db = OuiDb::load();
        let ble_db = BleCompanyDb::load();
        Self {
            names,
            oui_db,
            ble_db,
            bt_devices: HashMap::new(),
            wifi_devices: HashMap::new(),
            subghz_devices: HashMap::new(),
            bt_updates: 0,
            wifi_updates: 0,
            subghz_updates: 0,
            start_time: Instant::now(),
            tui: TuiState::new(),
            event_rx,
            event_tx,
        }
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;

            match self.event_rx.recv().await {
                Some(event) => self.handle_event(event),
                None => break,
            }

            if self.tui.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::BtUpdate(update) => {
                if !self.tui.paused {
                    store::apply_bt_update(&mut self.bt_devices, update);
                    self.bt_updates += 1;
                }
            }
            AppEvent::WifiApUpdate(update) => {
                if !self.tui.paused {
                    store::apply_wifi_ap_update(&mut self.wifi_devices, update);
                    self.wifi_updates += 1;
                }
            }
            AppEvent::WifiClientUpdate(update) => {
                if !self.tui.paused {
                    store::apply_wifi_client_update(&mut self.wifi_devices, update);
                    self.wifi_updates += 1;
                }
            }
            AppEvent::BtError(msg) => {
                let lower = msg.to_lowercase();
                let display = if lower.contains("org.bluez")
                    || lower.contains("bluetoothd")
                    || lower.contains("no adapter")
                    || lower.contains("no default adapter")
                {
                    format!("{} — try: sudo systemctl start bluetooth", msg)
                } else {
                    msg
                };
                self.tui.bt_status = Some(display);
            }
            AppEvent::WifiStatus(msg) => {
                self.tui.wifi_status = msg;
            }
            AppEvent::SubGhzSignal(sig) => {
                if !self.tui.paused {
                    store::apply_subghz_update(&mut self.subghz_devices, sig);
                    self.subghz_updates += 1;
                }
            }
            AppEvent::SubGhzStatus(msg) => {
                self.tui.subghz_status = Some(msg);
            }
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::Tick => {
                let now = Instant::now();
                self.bt_devices
                    .retain(|_, d| now.duration_since(d.last_seen).as_millis() < config::DROP_MS as u128);
                self.wifi_devices
                    .retain(|_, d| now.duration_since(d.last_seen).as_millis() < config::DROP_MS as u128);
                self.subghz_devices
                    .retain(|_, d| now.duration_since(d.last_seen).as_millis() < config::DROP_MS as u128);
            }
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.tui.wifi_password_modal.is_some() {
            self.handle_wifi_password_key(key);
            return;
        }

        if self.tui.editing {
            match key.code {
                KeyCode::Esc => {
                    self.tui.editing = false;
                    self.tui.edit_buffer.clear();
                    self.tui.frozen_order = None;
                }
                KeyCode::Enter => {
                    if let Some(ref id) = self.tui.selected_id {
                        let trimmed = self.tui.edit_buffer.trim().to_string();
                        if trimmed.is_empty() {
                            self.names.remove(id);
                        } else {
                            self.names.set(id, &trimmed);
                        }
                    }
                    self.tui.editing = false;
                    self.tui.edit_buffer.clear();
                    self.tui.frozen_order = None;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.tui.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        self.tui.edit_buffer.push(c);
                    }
                }
                _ => {}
            }
            return;
        }

        // Normal mode
        match key.code {
            KeyCode::Char('q') => self.tui.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.tui.should_quit = true;
            }
            KeyCode::Tab => self.tui.toggle_tab(),
            KeyCode::Up => {
                self.move_selection(-1);
            }
            KeyCode::Down => {
                self.move_selection(1);
            }
            KeyCode::Char('n') | KeyCode::Enter => {
                if let Some(ref id) = self.tui.selected_id {
                    self.tui.editing = true;
                    self.tui.edit_buffer = self.names.get(id).unwrap_or("").to_string();
                    let ids = self.visible_ids();
                    self.tui.frozen_order = Some(ids);
                }
            }
            KeyCode::Char('x') => {
                if let Some(ref id) = self.tui.selected_id.clone() {
                    self.names.remove(id);
                }
            }
            KeyCode::Char('s') => self.tui.cycle_sort(),
            KeyCode::Char('p') => self.tui.paused = !self.tui.paused,
            KeyCode::Char('w') if self.tui.tab == Tab::Wifi => self.start_wifi_connect(),
            KeyCode::Char('e') => self.export_current_tab(),
            KeyCode::Char('c') => {
                match self.tui.tab {
                    Tab::Bt => {
                        self.bt_devices.clear();
                        self.bt_updates = 0;
                    }
                    Tab::Wifi => {
                        self.wifi_devices.clear();
                        self.wifi_updates = 0;
                    }
                    Tab::SubGhz => {
                        self.subghz_devices.clear();
                        self.subghz_updates = 0;
                    }
                }
                self.tui.selected_id = None;
                self.tui.scroll_offset = 0;
            }
            _ => {}
        }
    }

    fn export_current_tab(&mut self) {
        let now = Instant::now();
        let names_lookup = |id: &str| self.names.get(id).map(|s| s.to_string());

        let (tab_name, payload, count) = match self.tui.tab {
            Tab::Bt => {
                let list = export::bt_to_export(
                    &self.bt_devices,
                    self.oui_db.as_ref(),
                    self.ble_db.as_ref(),
                    &names_lookup,
                    now,
                );
                let count = list.len();
                let payload = serde_json::to_string_pretty(&serde_json::json!({
                    "scan_type": "bt",
                    "exported_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    "device_count": count,
                    "devices": list,
                }));
                ("bt", payload, count)
            }
            Tab::Wifi => {
                let list = export::wifi_to_export(
                    &self.wifi_devices,
                    self.oui_db.as_ref(),
                    &names_lookup,
                    now,
                );
                let count = list.len();
                let payload = serde_json::to_string_pretty(&serde_json::json!({
                    "scan_type": "wifi",
                    "exported_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    "device_count": count,
                    "devices": list,
                }));
                ("wifi", payload, count)
            }
            Tab::SubGhz => {
                let list = export::subghz_to_export(
                    &self.subghz_devices,
                    &names_lookup,
                    now,
                );
                let count = list.len();
                let payload = serde_json::to_string_pretty(&serde_json::json!({
                    "scan_type": "subghz",
                    "exported_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    "device_count": count,
                    "devices": list,
                }));
                ("subghz", payload, count)
            }
        };

        let filename = export::export_filename(tab_name);
        let status = match payload {
            Ok(json) => match std::fs::write(&filename, json) {
                Ok(_) => format!("Exported {} devices → {}", count, filename),
                Err(e) => format!("Export failed (write): {}", e),
            },
            Err(e) => format!("Export failed (serialize): {}", e),
        };
        self.set_tab_status(status);
    }

    fn set_tab_status(&mut self, msg: String) {
        match self.tui.tab {
            Tab::Bt => self.tui.bt_status = Some(msg),
            Tab::Wifi => self.tui.wifi_status = Some(msg),
            Tab::SubGhz => self.tui.subghz_status = Some(msg),
        }
    }

    fn handle_wifi_password_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.tui.wifi_password_modal = None;
            }
            KeyCode::Enter => {
                if let Some(modal) = self.tui.wifi_password_modal.take() {
                    self.spawn_wifi_connect(modal.bssid, modal.ssid, Some(modal.buffer));
                }
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(ref mut m) = self.tui.wifi_password_modal {
                    m.buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    if let Some(ref mut m) = self.tui.wifi_password_modal {
                        m.buffer.push(c);
                    }
                }
            }
            _ => {}
        }
    }

    fn start_wifi_connect(&mut self) {
        let Some(ref id) = self.tui.selected_id else { return };
        let Some(device) = self.wifi_devices.get(id) else { return };

        if device.kind != WifiKind::Ap {
            self.tui.wifi_status = Some("Select an AP (not a client) to connect".into());
            return;
        }

        let bssid = device.mac.clone();
        let ssid = device.ssid.clone().unwrap_or_else(|| bssid.clone());
        let security = device.security.clone().unwrap_or_default();
        let is_open = security.is_empty() || security == "--" || security.eq_ignore_ascii_case("open");

        if is_open {
            self.spawn_wifi_connect(bssid, ssid, None);
        } else {
            self.tui.wifi_password_modal = Some(crate::tui::state::WifiPasswordModal {
                bssid,
                ssid,
                buffer: String::new(),
            });
        }
    }

    fn spawn_wifi_connect(&self, bssid: String, ssid: String, password: Option<String>) {
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            tx.send(AppEvent::WifiStatus(Some(format!("Connecting to {}…", ssid))))
                .await
                .ok();

            let mut cmd = tokio::process::Command::new("nmcli");
            cmd.args(["dev", "wifi", "connect", &bssid]);
            if let Some(ref pw) = password {
                cmd.arg("password").arg(pw);
            }

            let msg = match cmd.output().await {
                Ok(out) if out.status.success() => format!("Connected to {}", ssid),
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    let err = if err.is_empty() {
                        String::from_utf8_lossy(&out.stdout).trim().to_string()
                    } else {
                        err
                    };
                    format!("Connect failed: {}", err)
                }
                Err(e) => format!("Connect failed: {}", e),
            };
            tx.send(AppEvent::WifiStatus(Some(msg))).await.ok();
        });
    }

    /// Move selection up (-1) or down (+1) by finding current position in list
    fn move_selection(&mut self, delta: i32) {
        let (list_len, current_idx) = match self.tui.tab {
            Tab::Bt => {
                let list = self.build_bt_list();
                let idx = self.tui.selected_id.as_ref()
                    .and_then(|id| find_bt_index(&list, id))
                    .unwrap_or(0);
                (list.len(), idx)
            }
            Tab::Wifi => {
                let list = self.build_wifi_list();
                let idx = self.tui.selected_id.as_ref()
                    .and_then(|id| find_wifi_index(&list, id))
                    .unwrap_or(0);
                (list.len(), idx)
            }
            Tab::SubGhz => {
                let list = self.build_subghz_list();
                let idx = self.tui.selected_id.as_ref()
                    .and_then(|id| list.iter().position(|e| format!("{}", e.device.freq_hz) == *id))
                    .unwrap_or(0);
                (list.len(), idx)
            }
        };

        if list_len == 0 {
            return;
        }

        let new_idx = if delta < 0 {
            current_idx.saturating_sub((-delta) as usize)
        } else {
            (current_idx + delta as usize).min(list_len - 1)
        };

        match self.tui.tab {
            Tab::Bt => {
                let list = self.build_bt_list();
                self.tui.selected_id = bt_id(&list, new_idx);
            }
            Tab::Wifi => {
                let list = self.build_wifi_list();
                self.tui.selected_id = wifi_id(&list, new_idx);
            }
            Tab::SubGhz => {
                let list = self.build_subghz_list();
                self.tui.selected_id = list.get(new_idx).map(|e| format!("{}", e.device.freq_hz));
            }
        }
    }

    fn build_bt_list(&self) -> Vec<bt_table::BtListEntry> {
        let now = Instant::now();
        let mut active = Vec::new();
        let mut stale = Vec::new();

        for d in self.bt_devices.values() {
            if d.ema.is_none() {
                continue;
            }
            let age_ms = now.duration_since(d.last_seen).as_millis() as u64;
            if age_ms > config::DROP_MS {
                continue;
            }
            let is_stale = age_ms > config::BT_STALE_MS;
            let entry = bt_table::BtListEntry {
                device: d.clone(),
                stale: is_stale,
                custom_name: self.names.get(&d.address).map(|s| s.to_string()),
                vendor: oui::vendor_for_bt(
                    oui::BtIdentity {
                        mac: &d.address,
                        is_random: d.is_random,
                        company_id: d.company_id,
                        icon: d.icon.as_deref(),
                    },
                    self.oui_db.as_ref(),
                    self.ble_db.as_ref(),
                ),
            };
            if is_stale {
                stale.push(entry);
            } else {
                active.push(entry);
            }
        }

        let sort_fn = |a: &bt_table::BtListEntry, b: &bt_table::BtListEntry| -> std::cmp::Ordering {
            match self.tui.sort_mode {
                SortMode::Distance => {
                    let da = signal::bt_distance(a.device.ema.unwrap_or(-100.0), a.device.tx_power);
                    let db = signal::bt_distance(b.device.ema.unwrap_or(-100.0), b.device.tx_power);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortMode::Name => {
                    let na = a.custom_name.as_deref()
                        .or(a.device.name.as_deref())
                        .unwrap_or(&a.device.address);
                    let nb = b.custom_name.as_deref()
                        .or(b.device.name.as_deref())
                        .unwrap_or(&b.device.address);
                    na.cmp(nb)
                }
                _ => {
                    b.device.rssi.unwrap_or(-127).cmp(&a.device.rssi.unwrap_or(-127))
                }
            }
        };

        active.sort_by(sort_fn);
        stale.sort_by(sort_fn);
        active.extend(stale);
        active
    }

    fn build_wifi_list(&self) -> Vec<wifi_table::WifiListEntry> {
        let now = Instant::now();
        let mut active = Vec::new();
        let mut stale = Vec::new();

        for d in self.wifi_devices.values() {
            let has_sig = match d.kind {
                WifiKind::Ap => d.signal_pct.is_some(),
                WifiKind::Client => d.rssi.is_some(),
            };
            if !has_sig && d.ema.is_none() {
                continue;
            }
            let age_ms = now.duration_since(d.last_seen).as_millis() as u64;
            if age_ms > config::DROP_MS {
                continue;
            }
            let stale_thresh = match d.kind {
                WifiKind::Ap => config::AP_STALE_MS,
                WifiKind::Client => config::CLIENT_STALE_MS,
            };
            let is_stale = age_ms > stale_thresh;
            let entry = wifi_table::WifiListEntry {
                device: d.clone(),
                stale: is_stale,
                custom_name: self.names.get(&d.mac).map(|s| s.to_string()),
                vendor: oui::vendor_for_wifi(&d.mac, self.oui_db.as_ref()),
            };
            if is_stale {
                stale.push(entry);
            } else {
                active.push(entry);
            }
        }

        let get_signal = |d: &WifiDevice| -> f64 {
            match d.kind {
                WifiKind::Ap => d.ema.unwrap_or(0.0),
                WifiKind::Client => {
                    d.ema
                        .map(|e| signal::normalize_client_signal(e))
                        .unwrap_or(0.0)
                }
            }
        };

        let sort_fn = |a: &wifi_table::WifiListEntry, b: &wifi_table::WifiListEntry| -> std::cmp::Ordering {
            match self.tui.sort_mode {
                SortMode::Name => {
                    let na = a.custom_name.as_deref()
                        .or(a.device.ssid.as_deref())
                        .or(a.device.probing_for.as_deref())
                        .unwrap_or(&a.device.mac);
                    let nb = b.custom_name.as_deref()
                        .or(b.device.ssid.as_deref())
                        .or(b.device.probing_for.as_deref())
                        .unwrap_or(&b.device.mac);
                    na.cmp(nb)
                }
                SortMode::Type => {
                    if a.device.kind != b.device.kind {
                        if a.device.kind == WifiKind::Client {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Greater
                        }
                    } else {
                        get_signal(&b.device)
                            .partial_cmp(&get_signal(&a.device))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }
                _ => {
                    get_signal(&b.device)
                        .partial_cmp(&get_signal(&a.device))
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        };

        active.sort_by(sort_fn);
        stale.sort_by(sort_fn);
        active.extend(stale);
        active
    }

    fn visible_ids(&self) -> Vec<String> {
        match self.tui.tab {
            Tab::Bt => self.build_bt_list().iter().map(|e| e.device.address.clone()).collect(),
            Tab::Wifi => self.build_wifi_list().iter().map(|e| e.device.mac.clone()).collect(),
            Tab::SubGhz => self.build_subghz_list().iter().map(|e| format!("{}", e.device.freq_hz)).collect(),
        }
    }

    fn build_subghz_list(&self) -> Vec<subghz_table::SubGhzListEntry> {
        let now = Instant::now();
        let mut active = Vec::new();
        let mut stale = Vec::new();

        for d in self.subghz_devices.values() {
            let age_ms = now.duration_since(d.last_seen).as_millis() as u64;
            if age_ms > config::DROP_MS {
                continue;
            }
            let is_stale = age_ms > config::BT_STALE_MS;
            let key = format!("{}", d.freq_hz);
            let entry = subghz_table::SubGhzListEntry {
                device: d.clone(),
                stale: is_stale,
                custom_name: self.names.get(&key).map(|s| s.to_string()),
            };
            if is_stale { stale.push(entry); } else { active.push(entry); }
        }

        let sort_fn = |a: &subghz_table::SubGhzListEntry, b: &subghz_table::SubGhzListEntry| -> std::cmp::Ordering {
            match self.tui.sort_mode {
                SortMode::Name => {
                    let na = a.custom_name.as_deref().unwrap_or("");
                    let nb = b.custom_name.as_deref().unwrap_or("");
                    na.cmp(nb).then(a.device.freq_hz.cmp(&b.device.freq_hz))
                }
                _ => b.device.rssi.cmp(&a.device.rssi),
            }
        };

        active.sort_by(sort_fn);
        stale.sort_by(sort_fn);
        active.extend(stale);
        active
    }

    fn render(&mut self, f: &mut Frame) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs();
        let size = f.area();

        let has_status = match self.tui.tab {
            Tab::Bt => self.tui.bt_status.is_some(),
            Tab::Wifi => self.tui.wifi_status.is_some(),
            Tab::SubGhz => self.tui.subghz_status.is_some(),
        };
        let input_active = self.tui.input_active();
        let footer_height = if input_active { 0 } else if has_status { 2 } else { 1 };
        let modal_height = if input_active { 3 } else { 0 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(modal_height as u16),
                Constraint::Length(footer_height as u16),
            ])
            .split(size);

        match self.tui.tab {
            Tab::Bt => self.render_bt_tab(f, &chunks, now, elapsed),
            Tab::Wifi => self.render_wifi_tab(f, &chunks, now, elapsed),
            Tab::SubGhz => self.render_subghz_tab(f, &chunks, now, elapsed),
        }

        if self.tui.editing {
            self.render_naming_bar(f, chunks[3]);
        } else if self.tui.wifi_password_modal.is_some() {
            self.render_password_bar(f, chunks[3]);
        }

        let status_msg = match self.tui.tab {
            Tab::Bt => self.tui.bt_status.as_deref(),
            Tab::Wifi => self.tui.wifi_status.as_deref(),
            Tab::SubGhz => self.tui.subghz_status.as_deref(),
        };
        footer::render_footer(
            f,
            chunks[4],
            input_active,
            status_msg,
            true,
            self.tui.tab,
        );
    }

    fn render_top_header(&self, f: &mut Frame, area: Rect, updates: u64, elapsed: u64, active: usize, stale: usize) {
        header::render_header(f, area, &header::HeaderData {
            tab: self.tui.tab,
            elapsed_secs: elapsed,
            active,
            stale,
            updates,
            sort_mode: self.tui.sort_mode,
            paused: self.tui.paused,
            editing: self.tui.input_active(),
        });
    }

    fn render_empty_message(&self, f: &mut Frame, area: Rect, msg: &str) {
        let span = Span::styled(msg, Style::default().fg(Color::DarkGray));
        let area = Rect {
            x: area.x + 4,
            y: area.y + 1,
            width: area.width.saturating_sub(8),
            height: 1,
        };
        f.render_widget(ratatui::widgets::Paragraph::new(span), area);
    }

    fn render_bt_tab(&mut self, f: &mut Frame, chunks: &[Rect], now: Instant, elapsed: u64) {
        let max_rows = chunks[2].height as usize;
        let mut list = self.build_bt_list();

        if self.tui.selected_id.is_none() && !list.is_empty() {
            self.tui.selected_id = Some(list[0].device.address.clone());
        }

        let selected_idx = self.tui.selected_id.as_ref()
            .and_then(|id| find_bt_index(&list, id))
            .unwrap_or(0);

        let active_count = list.iter().filter(|e| !e.stale).count();
        let stale_count = list.iter().filter(|e| e.stale).count();

        self.render_top_header(f, chunks[0], self.bt_updates, elapsed, active_count, stale_count);
        bt_table::render_bt_header(f, chunks[1]);

        if self.tui.editing {
            if let Some(ref frozen) = self.tui.frozen_order {
                let by_id: HashMap<String, bt_table::BtListEntry> = list
                    .drain(..)
                    .map(|e| (e.device.address.clone(), e))
                    .collect();
                for id in frozen {
                    if let Some(entry) = by_id.get(id) {
                        list.push(bt_table::BtListEntry {
                            device: entry.device.clone(),
                            stale: entry.stale,
                            custom_name: entry.custom_name.clone(),
                            vendor: entry.vendor.clone(),
                        });
                    }
                }
            }
        }

        self.tui.ensure_visible(selected_idx, max_rows);
        let offset = self.tui.scroll_offset;

        for (i, entry) in list.iter().skip(offset).take(max_rows).enumerate() {
            let row_area = Rect {
                x: chunks[2].x,
                y: chunks[2].y + i as u16,
                width: chunks[2].width,
                height: 1,
            };
            let age_ms = now.duration_since(entry.device.last_seen).as_millis() as u64;
            bt_table::render_bt_row(f, row_area, entry, (i + offset) == selected_idx, age_ms);
        }

        if list.is_empty() {
            self.render_empty_message(f, chunks[2], "Scanning for devices...");
        }
    }

    fn render_wifi_tab(&mut self, f: &mut Frame, chunks: &[Rect], now: Instant, elapsed: u64) {
        let max_rows = chunks[2].height as usize;
        let list = self.build_wifi_list();

        if self.tui.selected_id.is_none() && !list.is_empty() {
            self.tui.selected_id = Some(list[0].device.mac.clone());
        }

        let selected_idx = self.tui.selected_id.as_ref()
            .and_then(|id| find_wifi_index(&list, id))
            .unwrap_or(0);

        let active_count = list.iter().filter(|e| !e.stale).count();
        let stale_count = list.iter().filter(|e| e.stale).count();

        self.render_top_header(f, chunks[0], self.wifi_updates, elapsed, active_count, stale_count);
        wifi_table::render_wifi_header(f, chunks[1]);

        self.tui.ensure_visible(selected_idx, max_rows);
        let offset = self.tui.scroll_offset;

        for (i, entry) in list.iter().skip(offset).take(max_rows).enumerate() {
            let row_area = Rect {
                x: chunks[2].x,
                y: chunks[2].y + i as u16,
                width: chunks[2].width,
                height: 1,
            };
            let age_ms = now.duration_since(entry.device.last_seen).as_millis() as u64;
            wifi_table::render_wifi_row(f, row_area, entry, (i + offset) == selected_idx, age_ms);
        }

        if list.is_empty() {
            self.render_empty_message(f, chunks[2], "Scanning... (monitor mode may need sudo)");
        }
    }

    fn render_subghz_tab(&mut self, f: &mut Frame, chunks: &[Rect], now: Instant, elapsed: u64) {
        let max_rows = chunks[2].height as usize;
        let list = self.build_subghz_list();

        if self.tui.selected_id.is_none() && !list.is_empty() {
            self.tui.selected_id = Some(format!("{}", list[0].device.freq_hz));
        }

        let selected_idx = self.tui.selected_id.as_ref()
            .and_then(|id| list.iter().position(|e| format!("{}", e.device.freq_hz) == *id))
            .unwrap_or(0);

        let active_count = list.iter().filter(|e| !e.stale).count();
        let stale_count = list.iter().filter(|e| e.stale).count();

        self.render_top_header(f, chunks[0], self.subghz_updates, elapsed, active_count, stale_count);
        subghz_table::render_subghz_header(f, chunks[1]);

        self.tui.ensure_visible(selected_idx, max_rows);
        let offset = self.tui.scroll_offset;

        for (i, entry) in list.iter().skip(offset).take(max_rows).enumerate() {
            let row_area = Rect {
                x: chunks[2].x,
                y: chunks[2].y + i as u16,
                width: chunks[2].width,
                height: 1,
            };
            let age_ms = now.duration_since(entry.device.last_seen).as_millis() as u64;
            subghz_table::render_subghz_row(f, row_area, entry, (i + offset) == selected_idx, age_ms);
        }

        if list.is_empty() {
            let msg = if self.tui.subghz_status.is_some() {
                "No signals detected"
            } else {
                "Scanning sub-1GHz bands..."
            };
            self.render_empty_message(f, chunks[2], msg);
        }
    }

    fn render_password_bar(&self, f: &mut Frame, area: Rect) {
        let Some(ref modal) = self.tui.wifi_password_modal else { return };
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let masked: String = "•".repeat(modal.buffer.chars().count());

        let spans = vec![
            Span::styled("  Password for ", Style::default().fg(Color::Magenta).bold()),
            Span::styled(format!("{} ", modal.ssid), Style::default().fg(Color::Cyan).bold()),
            Span::styled(format!("[{}]: ", modal.bssid), Style::default().fg(Color::DarkGray)),
            Span::styled(masked, Style::default().fg(Color::White).bold()),
            Span::styled("█", Style::default().fg(Color::Magenta)),
            Span::styled("    [Enter] connect  [Esc] cancel", Style::default().fg(Color::DarkGray)),
        ];
        f.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(spans)),
            inner,
        );
    }

    fn render_naming_bar(&self, f: &mut Frame, area: Rect) {
        let Some(ref id) = self.tui.selected_id else { return };
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let spans = vec![
            Span::styled("  Name device ", Style::default().fg(Color::Magenta).bold()),
            Span::styled(format!("[{}]: ", id), Style::default().fg(Color::DarkGray)),
            Span::styled(&self.tui.edit_buffer, Style::default().fg(Color::White).bold()),
            Span::styled("█", Style::default().fg(Color::Magenta)),
            Span::styled("    [Enter] save  [Esc] cancel", Style::default().fg(Color::DarkGray)),
        ];
        f.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(spans)),
            inner,
        );
    }
}
