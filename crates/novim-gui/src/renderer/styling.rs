//! Syntax highlighting, selection, search, cursor, and text utility functions.

use glyphon::Color;
use novim_core::buffer::BufferLike;
use novim_core::config::{self, SyntaxTheme};
use novim_core::highlight::HighlightGroup;
use novim_core::pane::{Pane, PaneContent};

use super::theme::*;

use novim_core::text_utils::snap_to_char_boundary;

// ── Syntax highlighting ───────────────────────────────────────────────────────

pub(super) fn apply_syntax_highlights(
    content: &str,
    spans: &[novim_core::highlight::HighlightSpan],
    theme: &SyntaxTheme,
) -> Vec<RichSpan> {
    if spans.is_empty() {
        return vec![RichSpan { text: content.to_string(), color: FG }];
    }

    let mut result = Vec::new();
    let mut pos = 0;

    for span in spans {
        let start = snap_to_char_boundary(content, span.start);
        let end = snap_to_char_boundary(content, span.end);

        if pos < start {
            result.push(RichSpan { text: content[pos..start].to_string(), color: FG });
        }
        if start < end {
            let color = highlight_group_color(span.group, theme);
            result.push(RichSpan { text: content[start..end].to_string(), color });
        }
        pos = end;
    }

    if pos < content.len() {
        result.push(RichSpan { text: content[pos..].to_string(), color: FG });
    }

    if result.is_empty() {
        vec![RichSpan { text: content.to_string(), color: FG }]
    } else {
        result
    }
}

pub(super) fn highlight_group_color(group: HighlightGroup, theme: &SyntaxTheme) -> Color {
    let Some(color_str) = group.theme_color(theme) else {
        return FG;
    };
    config_color_to_glyphon(config::parse_color(color_str))
}

// ── Selection ─────────────────────────────────────────────────────────────────

pub(super) fn highlight_with_selection(
    expanded: &str,
    line_num: usize,
    sel_start: novim_types::Position,
    sel_end: novim_types::Position,
    buf: &dyn BufferLike,
    syntax_theme: &SyntaxTheme,
) -> Vec<RichSpan> {
    // Get syntax-highlighted base spans
    let base = if let Some(hl) = buf.get_highlights(line_num) {
        apply_syntax_highlights(expanded, hl, syntax_theme)
    } else {
        vec![RichSpan { text: expanded.to_string(), color: FG }]
    };

    // Determine selection range on this line (in char columns)
    let char_count = expanded.chars().count();
    let line_start = if line_num == sel_start.line { sel_start.column } else if line_num > sel_start.line { 0 } else { return base; };
    let line_end = if line_num == sel_end.line { sel_end.column + 1 } else if line_num < sel_end.line { char_count } else { return base; };

    if line_num < sel_start.line || line_num > sel_end.line {
        return base;
    }

    // Apply selection color (just use a brighter color for selected text)
    let mut result = Vec::new();
    let mut pos = 0;
    for span in &base {
        let span_len = span.text.chars().count();
        let span_end = pos + span_len;

        if span_end <= line_start || pos >= line_end {
            // Entirely outside selection
            result.push(RichSpan { text: span.text.clone(), color: span.color });
        } else {
            // Overlaps selection — split
            let overlap_start = line_start.max(pos) - pos;
            let overlap_end = line_end.min(span_end) - pos;

            if overlap_start > 0 {
                let pre: String = span.text.chars().take(overlap_start).collect();
                result.push(RichSpan { text: pre, color: span.color });
            }
            let sel_text: String = span.text.chars().skip(overlap_start).take(overlap_end - overlap_start).collect();
            // Use a distinctive selection color (lighter foreground)
            result.push(RichSpan { text: sel_text, color: Color::rgb(255, 255, 255) });
            if overlap_end < span_len {
                let post: String = span.text.chars().skip(overlap_end).collect();
                result.push(RichSpan { text: post, color: span.color });
            }
        }
        pos = span_end;
    }
    result
}

// ── Search highlight ──────────────────────────────────────────────────────────

pub(super) fn apply_search_highlight(content: &str, spans: &[RichSpan], pattern: &str) -> Vec<RichSpan> {
    if pattern.is_empty() || content.is_empty() {
        return spans.to_vec();
    }

    // Find match positions in the content
    let lower_content = content.to_lowercase();
    let lower_pattern = pattern.to_lowercase();
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(pos) = lower_content[start..].find(&lower_pattern) {
        let abs = start + pos;
        matches.push((abs, abs + pattern.len()));
        start = abs + 1;
    }
    if matches.is_empty() {
        return spans.to_vec();
    }

    // Flatten spans into char-level colors, then apply search highlight
    let mut result = Vec::new();
    let mut char_pos = 0;
    for span in spans {
        let span_chars: Vec<char> = span.text.chars().collect();
        for &ch in &span_chars {
            let in_match = matches.iter().any(|(s, e)| char_pos >= *s && char_pos < *e);
            let color = if in_match { YELLOW } else { span.color };
            // Merge with previous span if same color
            if let Some(last) = result.last_mut() {
                let last: &mut RichSpan = last;
                if last.color.0 == color.0 {
                    last.text.push(ch);
                    char_pos += 1;
                    continue;
                }
            }
            result.push(RichSpan { text: ch.to_string(), color });
            char_pos += 1;
        }
    }
    result
}

// ── Cursor ────────────────────────────────────────────────────────────────────

/// Replace the character at `target_col` with inverse colors.
/// Since glyphon doesn't support per-glyph background colors, we render
/// a solid block character (█) in CURSOR_BG color to make the cursor visible.
pub(super) fn apply_cursor_to_spans(spans: &mut Vec<RichSpan>, target_col: usize) {
    let mut col = 0;
    for i in 0..spans.len() {
        let span_chars: Vec<char> = spans[i].text.chars().collect();
        let span_len = span_chars.len();
        if col + span_len > target_col {
            let local = target_col - col;
            let before: String = span_chars[..local].iter().collect();
            let cursor_ch = span_chars.get(local).copied().unwrap_or(' ');
            let after: String = span_chars[local + 1..].iter().collect();
            let orig_color = spans[i].color;

            let mut new_spans = Vec::new();
            if !before.is_empty() {
                new_spans.push(RichSpan { text: before, color: orig_color });
            }

            // Use a block character so the cursor is visible even on whitespace.
            // For non-space characters, overlay the block to show cursor position.
            if cursor_ch == ' ' || cursor_ch == '\0' {
                new_spans.push(RichSpan { text: "█".to_string(), color: CURSOR_BG });
            } else {
                // Show the character in dark color (simulating inverse video)
                new_spans.push(RichSpan { text: cursor_ch.to_string(), color: CURSOR_BG });
            }

            if !after.is_empty() {
                new_spans.push(RichSpan { text: after, color: orig_color });
            }

            spans.splice(i..i + 1, new_spans);
            return;
        }
        col += span_len;
    }
    // If target_col is past the end, append a cursor block
    spans.push(RichSpan { text: "█".to_string(), color: CURSOR_BG });
}

// ── Terminal cells ────────────────────────────────────────────────────────────

pub(super) fn cells_to_rich_spans(cells: &[novim_core::emulator::grid::Cell]) -> Vec<RichSpan> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_color = FG;
    let mut first = true;

    for cell in cells {
        let color = cell_color_to_glyphon(cell.fg);
        if first || color.0 != current_color.0 {
            if !current_text.is_empty() {
                spans.push(RichSpan { text: current_text.clone(), color: current_color });
                current_text.clear();
            }
            current_color = color;
            first = false;
        }
        current_text.push(cell.c);
    }
    if !current_text.is_empty() {
        spans.push(RichSpan { text: current_text, color: current_color });
    }
    spans
}

pub(super) fn cell_color_to_glyphon(c: novim_core::emulator::grid::CellColor) -> Color {
    use novim_core::emulator::grid::CellColor;
    match c {
        CellColor::Default => FG,
        CellColor::Indexed(idx) => indexed_256_to_rgb(idx),
        CellColor::Black => Color::rgb(0, 0, 0),
        CellColor::Red => Color::rgb(224, 108, 117),
        CellColor::Green => Color::rgb(152, 195, 121),
        CellColor::Yellow => Color::rgb(229, 192, 123),
        CellColor::Blue => Color::rgb(97, 175, 239),
        CellColor::Magenta => Color::rgb(198, 120, 221),
        CellColor::Cyan => Color::rgb(86, 182, 194),
        CellColor::White => Color::rgb(220, 220, 220),
        CellColor::BrightBlack => Color::rgb(128, 128, 128),
        CellColor::BrightRed => Color::rgb(240, 140, 140),
        CellColor::BrightGreen => Color::rgb(180, 220, 160),
        CellColor::BrightYellow => Color::rgb(240, 210, 150),
        CellColor::BrightBlue => Color::rgb(140, 200, 250),
        CellColor::BrightMagenta => Color::rgb(220, 160, 240),
        CellColor::BrightCyan => Color::rgb(130, 210, 220),
        CellColor::BrightWhite => Color::rgb(255, 255, 255),
        CellColor::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

pub(super) fn get_diag_marker(ws: &novim_core::editor::Workspace, pane: &Pane, line_num: usize) -> Option<RichSpan> {
    let uri = match &pane.content {
        PaneContent::Editor(buf) => buf.file_uri(),
        _ => None,
    }?;
    let diags = ws.diagnostics.get(&uri)?;
    let has_error = diags.iter().any(|d| d.line == line_num && d.severity == novim_core::lsp::DiagnosticSeverity::Error);
    let has_warning = diags.iter().any(|d| d.line == line_num && d.severity == novim_core::lsp::DiagnosticSeverity::Warning);
    if has_error {
        Some(RichSpan { text: "E".to_string(), color: DIAG_ERROR_FG })
    } else if has_warning {
        Some(RichSpan { text: "W".to_string(), color: DIAG_WARN_FG })
    } else {
        None
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

pub(super) use novim_core::text_utils::truncate_str;
