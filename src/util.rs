use unicode_width::UnicodeWidthChar;

/// Returns a string of exactly `width` terminal cells: truncates if `s` is
/// wider, right-pads with spaces if shorter. Counts display cells, not bytes
/// or chars, so emoji and CJK glyphs occupy the correct number of columns
/// and multi-byte UTF-8 boundaries are never split.
pub fn fixed_width(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut cells = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if cells + w > width {
            break;
        }
        out.push(c);
        cells += w;
    }
    for _ in cells..width {
        out.push(' ');
    }
    out
}
