use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::signal::{self, bt_distance, distance_info, trend_arrow, format_age, signal_bar};
use crate::store::BtDevice;
use crate::util::fixed_width;

pub struct BtListEntry {
    pub device: BtDevice,
    pub stale: bool,
    pub custom_name: Option<String>,
    pub vendor: Option<String>,
}

pub fn render_bt_header(f: &mut Frame, area: Rect) {
    let dim_bold = Style::default().fg(Color::DarkGray).bold();
    let header = format!(
        "    {:<3}{:<29}  {:<18}  {:>8}  {:>8}  {:<20}  {:<20}  {:>5}",
        "⬍", "Name", "MAC Address", "RSSI", "Avg", "Signal", "~Distance", "Seen"
    );
    f.render_widget(Paragraph::new(Span::styled(header, dim_bold)), area);
}

pub fn render_bt_row(f: &mut Frame, area: Rect, entry: &BtListEntry, selected: bool, now_ms: u64) {
    let d = &entry.device;
    let ema = d.ema.unwrap_or(-100.0);
    let dist = distance_info(bt_distance(ema, d.tx_power));
    let trend = trend_arrow(ema, d.prev_ema);
    let age = format_age(now_ms);

    let cursor = if selected { "▸ " } else { "  " };
    let cursor_style = if selected {
        Style::default().fg(Color::Cyan).bold()
    } else {
        Style::default()
    };

    let fallback_label = entry.vendor.as_deref().map(|v| format!("({})", v));

    if entry.stale {
        let dim = Style::default().fg(Color::DarkGray);
        let name = entry.custom_name.as_deref()
            .or(d.name.as_deref())
            .or(fallback_label.as_deref())
            .unwrap_or(&d.address);
        let line = format!(
            "{}? {}  {:<18}  {:>4} dBm  {:>4} dBm  {}  {:>7} {}  {:>5}",
            cursor,
            fixed_width(name, 29),
            &d.address,
            d.rssi.unwrap_or(0),
            ema as i16,
            "░".repeat(20),
            dist.text,
            fixed_width(&format!("({})", dist.tag), 12),
            age,
        );
        f.render_widget(Paragraph::new(Span::styled(line, dim)), area);
        return;
    }

    let bt_name = d.name.as_deref()
        .or(fallback_label.as_deref())
        .unwrap_or(&d.address);
    let name_is_vendor = d.name.is_none() && fallback_label.is_some();
    let rssi = d.rssi.unwrap_or(0);
    let color = signal::rssi_color(rssi);
    let (bar_fill, bar_color) = signal_bar(ema, -100.0, -40.0, 20);

    let mut spans = vec![
        Span::styled(cursor, cursor_style),
        Span::styled(format!("{} ", trend.symbol), Style::default().fg(trend.color)),
    ];

    let name_style = if name_is_vendor {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().bold()
    };
    if let Some(ref cn) = entry.custom_name {
        spans.push(Span::styled(
            fixed_width(cn, 16),
            Style::default().fg(Color::Magenta).bold(),
        ));
        spans.push(Span::styled(
            format!(" {}", fixed_width(bt_name, 12)),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::styled(
            fixed_width(bt_name, 29),
            name_style,
        ));
    }

    spans.extend([
        Span::styled(format!("  {:<18}", &d.address), Style::default().fg(Color::Cyan)),
        Span::styled(format!("  {:>4} dBm", rssi), Style::default().fg(color)),
        Span::styled(format!("  {:>4} dBm", ema as i16), Style::default().fg(color)),
        Span::raw("  "),
        Span::styled("█".repeat(bar_fill), Style::default().fg(bar_color)),
        Span::styled("░".repeat(20 - bar_fill), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("  {:>7} ", dist.text), Style::default().fg(dist.color)),
        Span::styled(fixed_width(&format!("({})", dist.tag), 12), Style::default().fg(dist.color)),
        Span::styled(format!("  {:>5}", age), Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
