use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::signal::{self, format_age, signal_bar};
use crate::store::SubGhzDevice;
use crate::util::fixed_width;

pub struct SubGhzListEntry {
    pub device: SubGhzDevice,
    pub stale: bool,
    pub custom_name: Option<String>,
}

pub fn render_subghz_header(f: &mut Frame, area: Rect) {
    let dim_bold = Style::default().fg(Color::DarkGray).bold();
    let header = format!(
        "    {:<3}{:<24}  {:>10}  {:>8}  {:>8}  {:<20}  {:<12}  {:>5}",
        "⬍", "Frequency / Name", "Band", "RSSI", "Avg", "Signal", "Detections", "Seen"
    );
    f.render_widget(Paragraph::new(Span::styled(header, dim_bold)), area);
}

pub fn render_subghz_row(f: &mut Frame, area: Rect, entry: &SubGhzListEntry, selected: bool, now_ms: u64) {
    let d = &entry.device;
    let ema = d.ema.unwrap_or(d.rssi as f64);
    let age = format_age(now_ms);
    let freq_mhz = d.freq_hz as f64 / 1_000_000.0;

    let cursor = if selected { "▸ " } else { "  " };
    let cursor_style = if selected {
        Style::default().fg(Color::Cyan).bold()
    } else {
        Style::default()
    };

    let trend = signal::trend_arrow(ema, d.prev_ema);
    let color = signal::rssi_color(d.rssi);
    let (bar_fill, bar_color) = signal_bar(ema, -100.0, -30.0, 20);

    let freq_str = if freq_mhz == freq_mhz.floor() {
        format!("{:.0} MHz", freq_mhz)
    } else {
        format!("{:.2} MHz", freq_mhz)
    };

    if entry.stale {
        let dim = Style::default().fg(Color::DarkGray);
        let name = entry.custom_name.as_deref().unwrap_or(&freq_str);
        let line = format!(
            "{}? {}  {:>10}  {:>4} dBm  {:>4} dBm  {}  {:>12}  {:>5}",
            cursor, fixed_width(name, 24), &d.band, d.rssi, ema as i16,
            "░".repeat(20), d.sample_count, age,
        );
        f.render_widget(Paragraph::new(Span::styled(line, dim)), area);
        return;
    }

    let mut spans = vec![
        Span::styled(cursor, cursor_style),
        Span::styled(format!("{} ", trend.symbol), Style::default().fg(trend.color)),
    ];

    // Name / frequency
    if let Some(ref cn) = entry.custom_name {
        spans.push(Span::styled(
            fixed_width(cn, 14),
            Style::default().fg(Color::Magenta).bold(),
        ));
        spans.push(Span::styled(
            format!(" {}", fixed_width(&freq_str, 9)),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::styled(
            fixed_width(&freq_str, 24),
            Style::default().fg(Color::Rgb(255, 140, 0)).bold(),
        ));
    }

    spans.extend([
        Span::styled(format!("  {:>10}", &d.band), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("  {:>4} dBm", d.rssi), Style::default().fg(color)),
        Span::styled(format!("  {:>4} dBm", ema as i16), Style::default().fg(color)),
        Span::raw("  "),
        Span::styled("█".repeat(bar_fill), Style::default().fg(bar_color)),
        Span::styled("░".repeat(20 - bar_fill), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("  {:>12}", format!("×{}", d.sample_count)), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("  {:>5}", age), Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
