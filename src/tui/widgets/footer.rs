use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render_footer(f: &mut Frame, area: Rect, editing: bool, wifi_status: Option<&str>, is_wifi: bool) {
    if editing {
        return;
    }

    let dim = Style::default().fg(Color::DarkGray);
    let hints = "[Tab] switch BT/WiFi  [q] quit  [↑↓] select  [n/Enter] name  [x] del name  [s] sort  [p] pause  [c] clear";

    let mut lines = vec![Line::from(Span::styled(hints, dim))];

    if is_wifi {
        if let Some(status) = wifi_status {
            lines.push(Line::from(Span::styled(
                format!("⚠ {}", status),
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
