use ratatui::style::Color;

use crate::config::EMA_ALPHA;

pub fn ema_update(prev: f64, new_val: f64) -> f64 {
    EMA_ALPHA * new_val + (1.0 - EMA_ALPHA) * prev
}

pub fn bt_distance(rssi: f64, tx_power: Option<i16>) -> f64 {
    let reference = match tx_power {
        Some(tp) if tp < 0 => tp as f64,
        _ => -59.0,
    };
    let n = 2.5;
    10f64.powf((reference - rssi) / (10.0 * n))
}

pub fn wifi_signal_to_dbm(pct: f64) -> f64 {
    (pct / 2.0) - 100.0
}

pub fn wifi_ap_distance(signal_pct: f64) -> f64 {
    let dbm = wifi_signal_to_dbm(signal_pct);
    let n = 3.0;
    let reference = -45.0;
    10f64.powf((reference - dbm) / (10.0 * n))
}

pub fn wifi_client_distance(rssi_dbm: f64) -> f64 {
    let n = 3.0;
    let reference = -45.0;
    10f64.powf((reference - rssi_dbm) / (10.0 * n))
}

pub fn normalize_client_signal(rssi: f64) -> f64 {
    ((rssi + 100.0) / 60.0 * 100.0).clamp(0.0, 100.0)
}

pub fn rssi_color(rssi: i16) -> Color {
    if rssi >= -60 {
        Color::Green
    } else if rssi >= -75 {
        Color::Yellow
    } else if rssi >= -85 {
        Color::Rgb(255, 165, 0)
    } else {
        Color::Red
    }
}

pub fn signal_pct_color(pct: f64) -> Color {
    if pct >= 70.0 {
        Color::Green
    } else if pct >= 50.0 {
        Color::Yellow
    } else if pct >= 30.0 {
        Color::Rgb(255, 165, 0)
    } else {
        Color::Red
    }
}

pub struct DistanceInfo {
    pub text: String,
    pub tag: &'static str,
    pub color: Color,
}

pub fn distance_info(meters: f64) -> DistanceInfo {
    let text = format!("{:.1}m", meters);
    if meters < 1.0 {
        DistanceInfo { text, tag: "immediate", color: Color::Green }
    } else if meters < 3.0 {
        DistanceInfo { text, tag: "very close", color: Color::Green }
    } else if meters < 8.0 {
        DistanceInfo { text, tag: "nearby", color: Color::Yellow }
    } else if meters < 15.0 {
        DistanceInfo { text, tag: "same room", color: Color::Rgb(255, 165, 0) }
    } else if meters < 30.0 {
        DistanceInfo { text, tag: "far", color: Color::Red }
    } else {
        DistanceInfo { text, tag: "very far", color: Color::Red }
    }
}

pub struct TrendInfo {
    pub symbol: &'static str,
    pub color: Color,
}

pub fn trend_arrow(ema: f64, prev_ema: Option<f64>) -> TrendInfo {
    let Some(prev) = prev_ema else {
        return TrendInfo { symbol: " ~", color: Color::DarkGray };
    };
    let diff = ema - prev;
    if diff > 3.0 {
        TrendInfo { symbol: "↑↑", color: Color::Green }
    } else if diff > 1.0 {
        TrendInfo { symbol: " ↑", color: Color::Green }
    } else if diff < -3.0 {
        TrendInfo { symbol: "↓↓", color: Color::Red }
    } else if diff < -1.0 {
        TrendInfo { symbol: " ↓", color: Color::Red }
    } else {
        TrendInfo { symbol: " ~", color: Color::DarkGray }
    }
}

pub fn format_age(millis: u64) -> String {
    let s = millis / 1000;
    if s < 1 {
        "<1s".into()
    } else if s < 60 {
        format!("{}s", s)
    } else {
        format!("{}m{}s", s / 60, s % 60)
    }
}

pub fn signal_bar(value: f64, min: f64, max: f64, width: usize) -> (usize, Color) {
    let pct = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let fill = (pct * width as f64).round() as usize;
    let color = if pct >= 0.6 {
        Color::Green
    } else if pct >= 0.4 {
        Color::Yellow
    } else if pct >= 0.2 {
        Color::Rgb(255, 165, 0)
    } else {
        Color::Red
    };
    (fill.min(width), color)
}
