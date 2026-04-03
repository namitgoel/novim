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
