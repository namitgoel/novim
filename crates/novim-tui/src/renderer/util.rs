//! Utility functions for line wrapping and layout calculations.

/// Tab colors -- each workspace gets a unique accent color.
pub(super) const TAB_COLORS: &[u8] = &[
    75,  // blue
    114, // green
    176, // purple
    174, // salmon
    180, // gold
    117, // teal
    210, // coral
    149, // lime
    139, // mauve
];

/// How many screen rows a line occupies when wrapped.
pub(super) fn wrapped_row_count(line: &str, width: usize) -> usize {
    if width == 0 || line.is_empty() { return 1; }
    let len = line.len();
    len.div_ceil(width).max(1)
}

/// Split a line into wrapped segments of at most `width` characters.
pub(super) fn wrap_line(line: &str, width: usize) -> Vec<&str> {
    if width == 0 || line.len() <= width {
        return vec![line];
    }
    let mut segments = Vec::new();
    let mut start = 0;
    while start < line.len() {
        let end = (start + width).min(line.len());
        segments.push(&line[start..end]);
        start = end;
    }
    if segments.is_empty() {
        segments.push("");
    }
    segments
}
