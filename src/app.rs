use std::collections::HashMap;
use std::io::Stdout;
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout};
use tokio::sync::mpsc;

use crate::config::{self, Config};
use crate::events::AppEvent;
use crate::names::NameStore;
use crate::signal;
use crate::store::{self, BtDevice, WifiDevice, WifiKind};
use crate::tui::state::{Tab, SortMode, TuiState};
use crate::tui::widgets::{header, footer, bt_table, wifi_table};

pub struct App {
    config: Arc<Config>,
    names: NameStore,
    bt_devices: HashMap<String, BtDevice>,
    wifi_devices: HashMap<String, WifiDevice>,
    bt_updates: u64,
    wifi_updates: u64,
    start_time: Instant,
    tui: TuiState,
    event_rx: mpsc::Receiver<AppEvent>,
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
    pub fn new(config: Arc<Config>, event_rx: mpsc::Receiver<AppEvent>) -> Self {
        let names = NameStore::load(&config.names_path);
        Self {
            config,
            names,
            bt_devices: HashMap::new(),
            wifi_devices: HashMap::new(),
            bt_updates: 0,
            wifi_updates: 0,
            start_time: Instant::now(),
            tui: TuiState::new(),
            event_rx,
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
                eprintln!("BT: {}", msg);
            }
            AppEvent::WifiStatus(msg) => {
                self.tui.wifi_status = Some(msg);
            }
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::Tick => {
                let now = Instant::now();
                self.bt_devices
                    .retain(|_, d| now.duration_since(d.last_seen).as_millis() < config::DROP_MS as u128);
                self.wifi_devices
                    .retain(|_, d| now.duration_since(d.last_seen).as_millis() < config::DROP_MS as u128);
            }
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
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
                }
                self.tui.selected_id = None;
                self.tui.scroll_offset = 0;
            }
            _ => {}
        }
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
        };

        if list_len == 0 {
            return;
        }

        let new_idx = if delta < 0 {
            current_idx.saturating_sub((-delta) as usize)
        } else {
            (current_idx + delta as usize).min(list_len - 1)
        };

        // Set selected_id to the device at the new index
        match self.tui.tab {
            Tab::Bt => {
                let list = self.build_bt_list();
                self.tui.selected_id = bt_id(&list, new_idx);
            }
            Tab::Wifi => {
                let list = self.build_wifi_list();
                self.tui.selected_id = wifi_id(&list, new_idx);
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
        }
    }

    fn render(&mut self, f: &mut Frame) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs();
        let size = f.area();

        let footer_height = if self.tui.editing { 0 } else if self.tui.tab == Tab::Wifi && self.tui.wifi_status.is_some() { 2 } else { 1 };
        let naming_height = if self.tui.editing { 3 } else { 0 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(naming_height as u16),
                Constraint::Length(footer_height as u16),
            ])
            .split(size);

        let max_rows = chunks[2].height as usize;

        // Build list and resolve selection index from selected_id
        match self.tui.tab {
            Tab::Bt => {
                let mut list = self.build_bt_list();

                // Auto-select first if nothing selected
                if self.tui.selected_id.is_none() && !list.is_empty() {
                    self.tui.selected_id = Some(list[0].device.address.clone());
                }

                // Find index of selected device
                let selected_idx = self.tui.selected_id.as_ref()
                    .and_then(|id| find_bt_index(&list, id))
                    .unwrap_or(0);

                let (active_count, stale_count) = (
                    list.iter().filter(|e| !e.stale).count(),
                    list.iter().filter(|e| e.stale).count(),
                );

                // Header
                header::render_header(f, chunks[0], &header::HeaderData {
                    tab: self.tui.tab,
                    elapsed_secs: elapsed,
                    active: active_count,
                    stale: stale_count,
                    updates: self.bt_updates,
                    sort_mode: self.tui.sort_mode,
                    paused: self.tui.paused,
                    editing: self.tui.editing,
                });

                bt_table::render_bt_header(f, chunks[1]);

                // Apply frozen order during editing
                if self.tui.editing {
                    if let Some(ref frozen) = self.tui.frozen_order {
                        let by_id: HashMap<String, bt_table::BtListEntry> = list
                            .drain(..)
                            .map(|e| {
                                let key = e.device.address.clone();
                                (key, e)
                            })
                            .collect();
                        for id in frozen {
                            if let Some(entry) = by_id.get(id) {
                                list.push(bt_table::BtListEntry {
                                    device: entry.device.clone(),
                                    stale: entry.stale,
                                    custom_name: entry.custom_name.clone(),
                                });
                            }
                        }
                    }
                }

                // Scrolling
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
                    let is_selected = (i + offset) == selected_idx;
                    bt_table::render_bt_row(f, row_area, entry, is_selected, age_ms);
                }

                if list.is_empty() {
                    let msg = Span::styled("Scanning for devices...", Style::default().fg(Color::DarkGray));
                    let area = Rect { x: chunks[2].x + 4, y: chunks[2].y + 1, width: chunks[2].width.saturating_sub(8), height: 1 };
                    f.render_widget(ratatui::widgets::Paragraph::new(msg), area);
                }
            }
            Tab::Wifi => {
                let list = self.build_wifi_list();

                if self.tui.selected_id.is_none() && !list.is_empty() {
                    self.tui.selected_id = Some(list[0].device.mac.clone());
                }

                let selected_idx = self.tui.selected_id.as_ref()
                    .and_then(|id| find_wifi_index(&list, id))
                    .unwrap_or(0);

                let (active_count, stale_count) = (
                    list.iter().filter(|e| !e.stale).count(),
                    list.iter().filter(|e| e.stale).count(),
                );

                header::render_header(f, chunks[0], &header::HeaderData {
                    tab: self.tui.tab,
                    elapsed_secs: elapsed,
                    active: active_count,
                    stale: stale_count,
                    updates: self.wifi_updates,
                    sort_mode: self.tui.sort_mode,
                    paused: self.tui.paused,
                    editing: self.tui.editing,
                });

                wifi_table::render_wifi_header(f, chunks[1]);

                // Scrolling
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
                    let is_selected = (i + offset) == selected_idx;
                    wifi_table::render_wifi_row(f, row_area, entry, is_selected, age_ms);
                }

                if list.is_empty() {
                    let msg = Span::styled("Scanning... (monitor mode may need sudo)", Style::default().fg(Color::DarkGray));
                    let area = Rect { x: chunks[2].x + 4, y: chunks[2].y + 1, width: chunks[2].width.saturating_sub(8), height: 1 };
                    f.render_widget(ratatui::widgets::Paragraph::new(msg), area);
                }
            }
        }

        // Naming bar
        if self.tui.editing {
            if let Some(ref id) = self.tui.selected_id {
                let block = ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta));
                let inner = block.inner(chunks[3]);
                f.render_widget(block, chunks[3]);

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

        // Footer
        footer::render_footer(
            f,
            chunks[4],
            self.tui.editing,
            self.tui.wifi_status.as_deref(),
            self.tui.tab == Tab::Wifi,
        );
    }
}
