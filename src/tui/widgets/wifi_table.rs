use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::signal::{self, wifi_ap_distance, wifi_client_distance, distance_info, trend_arrow, format_age, signal_bar};
use crate::store::{WifiDevice, WifiKind};

pub struct WifiListEntry {
    pub device: WifiDevice,
    pub stale: bool,
    pub custom_name: Option<String>,
}

pub fn render_wifi_header(f: &mut Frame, area: Rect) {
    let dim_bold = Style::default().fg(Color::DarkGray).bold();
    let header = format!(
        "    {:<3}{:<27}  {:<18}  {:>8}  {:>8}  {:<20}  {:>7}  {:<25}  {:>5}",
        "⬍", "Type / Name", "MAC / BSSID", "Signal", "Avg", "Strength", "~Dist", "Info", "Seen"
    );
    f.render_widget(Paragraph::new(Span::styled(header, dim_bold)), area);
}

pub fn render_wifi_row(f: &mut Frame, area: Rect, entry: &WifiListEntry, selected: bool, now_ms: u64) {
    let d = &entry.device;
    let is_ap = d.kind == WifiKind::Ap;
    let ema = d.ema.unwrap_or(0.0);
    let age = format_age(now_ms);

    let dist = if is_ap {
        distance_info(wifi_ap_distance(ema))
    } else if d.ema.is_some() {
        distance_info(wifi_client_distance(ema))
    } else {
        crate::signal::DistanceInfo {
            text: "?".into(),
            tag: "",
            color: Color::DarkGray,
        }
    };

    let cursor = if selected { "▸ " } else { "  " };
    let cursor_style = if selected {
        Style::default().fg(Color::Cyan).bold()
    } else {
        Style::default()
    };

    let display_name = if is_ap {
        d.ssid.as_deref().unwrap_or("(hidden)")
    } else {
        &d.mac
    };

    if entry.stale {
        let dim = Style::default().fg(Color::DarkGray);
        let kind_tag = if is_ap { "AP " } else { "📱 " };
        let sig = if is_ap {
            format!("{}%", d.signal_pct.unwrap_or(0.0) as i32)
        } else {
            d.rssi.map(|r| format!("{}dBm", r)).unwrap_or("?".into())
        };
        let name = entry.custom_name.as_deref().unwrap_or(display_name);
        let line = format!(
            "{}? {}{:<24}  {:<18}  {:>7}  {}  {:>7}  {:>5}",
            cursor, kind_tag, truncate(name, 24), &d.mac, sig, "░".repeat(20), dist.text, age,
        );
        f.render_widget(Paragraph::new(Span::styled(line, dim)), area);
        return;
    }

    let trend = trend_arrow(ema, d.prev_ema);

    let mut spans = vec![
        Span::styled(cursor, cursor_style),
        Span::styled(format!("{} ", trend.symbol), Style::default().fg(trend.color)),
    ];

    // Kind tag
    if is_ap {
        spans.push(Span::styled("AP ", Style::default().fg(Color::Blue)));
    } else {
        spans.push(Span::styled("📱 ", Style::default().fg(Color::Yellow)));
    }

    // Name columns
    if let Some(ref cn) = entry.custom_name {
        spans.push(Span::styled(
            format!("{:<14}", truncate(cn, 14)),
            Style::default().fg(Color::Magenta).bold(),
        ));
        spans.push(Span::styled(
            format!(" {:<9}", truncate(display_name, 9)),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        let name_style = if display_name == "(hidden)" {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().bold()
        };
        spans.push(Span::styled(
            format!("{:<24}", truncate(display_name, 24)),
            name_style,
        ));
    }

    // MAC
    spans.push(Span::styled(
        format!("  {:<18}", &d.mac),
        Style::default().fg(Color::Cyan),
    ));

    // Signal display
    if is_ap {
        let sig = d.signal_pct.unwrap_or(0.0);
        let color = signal::signal_pct_color(sig);
        spans.push(Span::styled(format!("  {:>4}%", sig as i32), Style::default().fg(color)));
        spans.push(Span::styled(format!("  {:>4}%", ema as i32), Style::default().fg(color)));
        spans.push(Span::raw("  "));
        let (fill, bar_color) = signal_bar(ema, 0.0, 100.0, 20);
        spans.push(Span::styled("█".repeat(fill), Style::default().fg(bar_color)));
        spans.push(Span::styled("░".repeat(20 - fill), Style::default().fg(Color::DarkGray)));
    } else {
        let rssi = d.rssi.unwrap_or(0);
        let color = signal::rssi_color(rssi);
        spans.push(Span::styled(
            format!("  {:>4}dBm", if d.rssi.is_some() { format!("{}", rssi) } else { "?".into() }),
            Style::default().fg(color),
        ));
        spans.push(Span::styled(
            format!(" {:>4}dBm", if d.ema.is_some() { format!("{}", ema as i16) } else { "?".into() }),
            Style::default().fg(color),
        ));
        spans.push(Span::raw("  "));
        if d.ema.is_some() {
            let (fill, bar_color) = signal_bar(ema, -100.0, -40.0, 20);
            spans.push(Span::styled("█".repeat(fill), Style::default().fg(bar_color)));
            spans.push(Span::styled("░".repeat(20 - fill), Style::default().fg(Color::DarkGray)));
        } else {
            spans.push(Span::styled("░".repeat(20), Style::default().fg(Color::DarkGray)));
        }
    }

    // Distance
    spans.push(Span::styled(format!("  {:>7}", dist.text), Style::default().fg(dist.color)));

    // Extra info
    if is_ap {
        let ch = d.channel.as_deref().unwrap_or("");
        let freq = d.freq.as_deref().unwrap_or("");
        let sec = d.security.as_deref().unwrap_or("");
        spans.push(Span::styled(
            format!("  {:<13}  {:<10}", format!("{} {}", ch, freq), sec),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        let probe = d.probing_for.as_deref().map(|s| format!("→ {}", s)).unwrap_or_default();
        spans.push(Span::styled(
            format!("  {:<25}", truncate(&probe, 25)),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Age
    spans.push(Span::styled(format!("  {:>5}", age), Style::default().fg(Color::DarkGray)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}
