use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::state::Tab;

pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    input_active: bool,
    status: Option<&str>,
    show_status: bool,
    tab: Tab,
) {
    if input_active {
        return;
    }

    let dim = Style::default().fg(Color::DarkGray);
    let base = "[Tab] switch tabs  [q] quit  [↑↓] select  [n] name  [x] del name  [s] sort  [p] pause  [c] clear  [e] export";
    let hints = if tab == Tab::Wifi {
        format!("{}  [w] connect", base)
    } else {
        base.to_string()
    };

    let mut lines = vec![Line::from(Span::styled(hints, dim))];

    if show_status {
        if let Some(status) = status {
            lines.push(Line::from(Span::styled(
                format!("⚠ {}", status),
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
