use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::tui::state::{Tab, SortMode};

pub struct HeaderData {
    pub tab: Tab,
    pub elapsed_secs: u64,
    pub active: usize,
    pub stale: usize,
    pub updates: u64,
    pub sort_mode: SortMode,
    pub paused: bool,
    pub editing: bool,
}

pub fn render_header(f: &mut Frame, area: Rect, data: &HeaderData) {
    let border_color = if data.editing {
        Color::Magenta
    } else {
        match data.tab {
            Tab::Bt => Color::Cyan,
            Tab::Wifi => Color::Green,
            Tab::SubGhz => Color::Rgb(255, 140, 0),
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let mins = data.elapsed_secs / 60;
    let secs = data.elapsed_secs % 60;

    let title_color = match data.tab {
        Tab::Bt => Color::Cyan,
        Tab::Wifi => Color::Green,
        Tab::SubGhz => Color::Rgb(255, 140, 0),
    };

    let bt_style = if data.tab == Tab::Bt {
        Style::default().fg(Color::Cyan).bold().reversed()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let wifi_style = if data.tab == Tab::Wifi {
        Style::default().fg(Color::Green).bold().reversed()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let subghz_style = if data.tab == Tab::SubGhz {
        Style::default().fg(Color::Rgb(255, 140, 0)).bold().reversed()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let dim = Style::default().fg(Color::DarkGray);

    let mut spans = vec![
        Span::styled("  airprowl  ", Style::default().fg(title_color).bold()),
        Span::styled(" BT ", bt_style),
        Span::styled(" │ ", dim),
        Span::styled(" WiFi ", wifi_style),
        Span::styled(" │ ", dim),
        Span::styled(" Sub-GHz ", subghz_style),
        Span::styled("    ", dim),
        Span::styled("⏱  ", dim),
        Span::raw(format!("{:02}:{:02}", mins, secs)),
        Span::styled("  │  ", dim),
        Span::styled(format!("📡 {}", data.active), Style::default().fg(Color::Green)),
        Span::styled(" active  ", dim),
        Span::styled(format!("👻 {} stale", data.stale), dim),
        Span::styled("  │  ", dim),
        Span::styled(format!("🔄 {}", data.updates), dim),
        Span::styled("  │  sort: ", dim),
        Span::styled(format!("{}", data.sort_mode), Style::default().fg(Color::Yellow)),
    ];

    if data.paused {
        spans.push(Span::styled(" ⏸ PAUSED", Style::default().fg(Color::Red)));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(block);
    f.render_widget(paragraph, area);
}
