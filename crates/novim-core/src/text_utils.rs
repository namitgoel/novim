//! Shared text utilities used by both TUI and GUI renderers.

use std::borrow::Cow;

/// Expand tab characters to spaces based on tab width.
/// Returns a borrowed reference when no tabs are present (zero-alloc fast path).
pub fn expand_tabs(line: &str, tab_width: usize) -> Cow<'_, str> {
    if !line.contains('\t') {
        return Cow::Borrowed(line);
    }
    let mut result = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces {
                result.push(' ');
            }
            col += spaces;
        } else {
            result.push(c);
            col += 1;
        }
    }
    Cow::Owned(result)
}

/// Calculate display column accounting for tab expansion.
pub fn display_col(line: &str, cursor_col: usize, tab_width: usize) -> usize {
    let mut display = 0;
    for (i, c) in line.chars().enumerate() {
        if i >= cursor_col {
            break;
        }
        if c == '\t' {
            display += tab_width - (display % tab_width);
        } else {
            display += 1;
        }
    }
    display
}

/// Snap a byte offset to the nearest char boundary (rounding down).
pub fn snap_to_char_boundary(s: &str, byte_idx: usize) -> usize {
    let idx = byte_idx.min(s.len());
    if s.is_char_boundary(idx) {
        return idx;
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Convert a character column index to a byte offset in a string.
pub fn char_col_to_byte(s: &str, col: usize) -> usize {
    s.char_indices()
        .nth(col)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// Truncate a string to at most `max_chars` characters.
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        chars[..max_chars].iter().collect()
    }
}

/// How many screen rows a line occupies when wrapped (char-aware).
pub fn wrapped_row_count(line: &str, width: usize) -> usize {
    if width == 0 || line.is_empty() { return 1; }
    let char_count = line.chars().count();
    char_count.div_ceil(width).max(1)
}

/// Split a line into wrapped segments of at most `width` characters (char-aware).
pub fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let char_count = line.chars().count();
    if width == 0 || char_count <= width {
        return vec![line.to_string()];
    }
    let chars: Vec<char> = line.chars().collect();
    let mut segments = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        segments.push(chars[start..end].iter().collect::<String>());
        start = end;
    }
    if segments.is_empty() {
        segments.push(String::new());
    }
    segments
}
