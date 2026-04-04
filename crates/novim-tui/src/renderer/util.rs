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

/// How many screen rows a line occupies when wrapped (char-aware).
pub(super) fn wrapped_row_count(line: &str, width: usize) -> usize {
    if width == 0 || line.is_empty() { return 1; }
    let char_count = line.chars().count();
    char_count.div_ceil(width).max(1)
}

/// Split a line into wrapped segments of at most `width` characters (char-aware).
pub(super) fn wrap_line(line: &str, width: usize) -> Vec<String> {
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
