#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Bt,
    Wifi,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Signal,
    Distance,
    Name,
    Type,
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SortMode::Signal => write!(f, "signal"),
            SortMode::Distance => write!(f, "distance"),
            SortMode::Name => write!(f, "name"),
            SortMode::Type => write!(f, "type"),
        }
    }
}

pub struct TuiState {
    pub tab: Tab,
    pub sort_mode: SortMode,
    pub selected_id: Option<String>,
    pub scroll_offset: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub frozen_order: Option<Vec<String>>,
    pub paused: bool,
    pub should_quit: bool,
    pub wifi_status: Option<String>,
}

impl TuiState {
    pub fn new() -> Self {
        Self {
            tab: Tab::Bt,
            sort_mode: SortMode::Signal,
            selected_id: None,
            scroll_offset: 0,
            editing: false,
            edit_buffer: String::new(),
            frozen_order: None,
            paused: false,
            should_quit: false,
            wifi_status: None,
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort_mode = match self.tab {
            Tab::Bt => match self.sort_mode {
                SortMode::Signal => SortMode::Distance,
                SortMode::Distance => SortMode::Name,
                _ => SortMode::Signal,
            },
            Tab::Wifi => match self.sort_mode {
                SortMode::Signal => SortMode::Name,
                SortMode::Name => SortMode::Type,
                _ => SortMode::Signal,
            },
        };
    }

    pub fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Bt => Tab::Wifi,
            Tab::Wifi => Tab::Bt,
        };
        self.selected_id = None;
        self.scroll_offset = 0;
        self.sort_mode = SortMode::Signal;
    }

    /// Adjust scroll_offset so selected_index is visible within max_rows
    pub fn ensure_visible(&mut self, selected_index: usize, max_rows: usize) {
        if max_rows == 0 {
            return;
        }
        if selected_index < self.scroll_offset {
            self.scroll_offset = selected_index;
        } else if selected_index >= self.scroll_offset + max_rows {
            self.scroll_offset = selected_index - max_rows + 1;
        }
    }
}
