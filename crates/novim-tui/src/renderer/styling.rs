//! Syntax highlighting, selection styling, and color conversion utilities.

use novim_core::config;
use novim_core::emulator::grid::{CellAttrs, CellColor};
use novim_core::highlight::HighlightGroup;
use novim_core::text_utils::{snap_to_char_boundary, char_col_to_byte};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Apply syntax highlight spans to a line.
pub(super) fn apply_highlights(content: &str, spans: &[novim_core::highlight::HighlightSpan], theme: &config::SyntaxTheme) -> Vec<Span<'static>> {
    if spans.is_empty() {
        return vec![Span::raw(content.to_string())];
    }

    let mut result = Vec::new();
    let mut pos = 0;

    for span in spans {
        let start = snap_to_char_boundary(content, span.start);
        let end = snap_to_char_boundary(content, span.end);

        // Text before this span (unhighlighted)
        if pos < start {
            result.push(Span::raw(content[pos..start].to_string()));
        }

        // Highlighted span
        if start < end {
            let style = highlight_group_to_style(span.group, theme);
            result.push(Span::styled(content[start..end].to_string(), style));
        }

        pos = end;
    }

    // Remaining text after last span
    if pos < content.len() {
        result.push(Span::raw(content[pos..].to_string()));
    }

    if result.is_empty() {
        vec![Span::raw(content.to_string())]
    } else {
        result
    }
}

pub(super) fn highlight_group_to_style(group: HighlightGroup, theme: &config::SyntaxTheme) -> Style {
    let Some(color_str) = group.theme_color(theme) else {
        return Style::default();
    };
    let color = config_color_to_ratatui(config::parse_color(color_str));
    let mut style = Style::default().fg(color);
    if group.is_bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Render a line with selection highlighting.
pub(super) fn styled_line_with_selection(
    content: &str,
    line_num: usize,
    sel: novim_types::Selection,
) -> Vec<Span<'static>> {
    let (start, end) = sel.ordered();
    let sel_style = Style::default().bg(Color::DarkGray).fg(Color::White);

    // Check if this line intersects the selection
    if line_num < start.line || line_num > end.line {
        return vec![Span::raw(content.to_string())];
    }

    let line_len = content.len();
    // Convert char columns to byte offsets safely
    let sel_start_byte = if line_num == start.line {
        char_col_to_byte(content, start.column)
    } else {
        0
    };
    let sel_end_byte = if line_num == end.line {
        char_col_to_byte(content, end.column + 1).min(line_len)
    } else {
        line_len
    };

    let sel_start_byte = sel_start_byte.min(line_len);
    let sel_end_byte = sel_end_byte.min(line_len);

    let mut spans = Vec::new();

    // Before selection
    if sel_start_byte > 0 {
        spans.push(Span::raw(content[..sel_start_byte].to_string()));
    }

    // Selected portion
    if sel_start_byte < sel_end_byte {
        spans.push(Span::styled(
            content[sel_start_byte..sel_end_byte].to_string(),
            sel_style,
        ));
    }

    // After selection
    if sel_end_byte < line_len {
        spans.push(Span::raw(content[sel_end_byte..].to_string()));
    }

    if spans.is_empty() {
        vec![Span::raw(content.to_string())]
    } else {
        spans
    }
}

/// Apply diagnostic underlines on top of existing styled spans.
/// Preserves original syntax colors, adds underline on diagnostic ranges.
pub(super) fn apply_diagnostic_highlights<'a>(
    content: &str,
    existing_spans: &[Span<'a>],
    diags: &[&novim_core::lsp::Diagnostic],
) -> Vec<Span<'static>> {
    let mut ranges: Vec<(usize, usize, novim_core::lsp::DiagnosticSeverity)> = diags
        .iter()
        .map(|d| {
            let start = char_col_to_byte(content, d.col_start).min(content.len());
            let end = char_col_to_byte(content, d.col_end).min(content.len());
            (start, end, d.severity)
        })
        .filter(|(s, e, _)| s < e)
        .collect();
    ranges.sort_by_key(|(s, _, _)| *s);

    if ranges.is_empty() {
        return existing_spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect();
    }

    let mut result = Vec::new();
    let mut char_pos = 0;

    for span in existing_spans {
        let span_text = span.content.to_string();
        let span_start = char_pos;
        let span_end = char_pos + span_text.len();
        let base_style = span.style;

        let mut pos_in_span = 0;

        for (d_start, d_end, severity) in &ranges {
            if *d_end <= span_start || *d_start >= span_end {
                continue;
            }

            let local_start = d_start.saturating_sub(span_start).min(span_text.len());
            let local_end = d_end.saturating_sub(span_start).min(span_text.len());

            if pos_in_span < local_start {
                result.push(Span::styled(span_text[pos_in_span..local_start].to_string(), base_style));
            }

            if local_start < local_end {
                let diag_modifier = match severity {
                    novim_core::lsp::DiagnosticSeverity::Error => base_style.fg(Color::Red).add_modifier(Modifier::UNDERLINED),
                    novim_core::lsp::DiagnosticSeverity::Warning => base_style.fg(Color::Yellow).add_modifier(Modifier::UNDERLINED),
                    _ => base_style.add_modifier(Modifier::UNDERLINED),
                };
                result.push(Span::styled(span_text[local_start..local_end].to_string(), diag_modifier));
            }

            pos_in_span = local_end;
        }

        if pos_in_span < span_text.len() {
            result.push(Span::styled(span_text[pos_in_span..].to_string(), base_style));
        }

        char_pos = span_end;
    }

    if result.is_empty() {
        existing_spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect()
    } else {
        result
    }
}

/// Apply search match highlighting on top of existing styled spans.
/// Preserves original syntax colors, adds background highlight on matches.
pub(super) fn apply_search_highlights<'a>(
    content: &str,
    existing_spans: &[Span<'a>],
    pattern: &str,
) -> Vec<Span<'static>> {
    // Find all match positions in the line
    let matches: Vec<(usize, usize)> = content
        .match_indices(pattern)
        .map(|(i, p)| (i, i + p.len()))
        .collect();

    if matches.is_empty() {
        return existing_spans.iter().map(|s| {
            Span::styled(s.content.to_string(), s.style)
        }).collect();
    }

    let match_bg = Color::Indexed(58); // dark yellow background

    // Walk through existing spans and split them at match boundaries
    let mut result = Vec::new();
    let mut char_pos = 0;

    for span in existing_spans {
        let span_text = span.content.to_string();
        let span_start = char_pos;
        let span_end = char_pos + span_text.len();
        let base_style = span.style;

        let mut pos_in_span = 0;

        for &(m_start, m_end) in &matches {
            // Skip matches outside this span
            if m_end <= span_start || m_start >= span_end {
                continue;
            }

            // Clamp match to this span
            let local_start = m_start.saturating_sub(span_start).min(span_text.len());
            let local_end = m_end.saturating_sub(span_start).min(span_text.len());

            // Text before match (keep original style)
            if pos_in_span < local_start {
                result.push(Span::styled(
                    span_text[pos_in_span..local_start].to_string(),
                    base_style,
                ));
            }

            // Match text (original style + background)
            if local_start < local_end {
                result.push(Span::styled(
                    span_text[local_start..local_end].to_string(),
                    base_style.bg(match_bg),
                ));
            }

            pos_in_span = local_end;
        }

        // Remaining text after last match in this span
        if pos_in_span < span_text.len() {
            result.push(Span::styled(
                span_text[pos_in_span..].to_string(),
                base_style,
            ));
        }

        char_pos = span_end;
    }

    if result.is_empty() {
        existing_spans.iter().map(|s| {
            Span::styled(s.content.to_string(), s.style)
        }).collect()
    } else {
        result
    }
}

/// Highlight a single character at a display column for secondary cursors.
pub(super) fn apply_cursor_highlight<'a>(line: &str, spans: &[Span<'a>], display_col: usize) -> Vec<Span<'a>> {
    if display_col >= line.len() {
        return spans.to_vec();
    }
    let cursor_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let mut result = Vec::new();
    let mut pos = 0;
    for span in spans {
        let span_len = span.content.len();
        if pos + span_len <= display_col || pos > display_col {
            result.push(span.clone());
        } else {
            let local = display_col - pos;
            if local > 0 {
                result.push(Span::styled(span.content[..local].to_string(), span.style));
            }
            let cursor_char = &span.content[local..local + 1];
            result.push(Span::styled(cursor_char.to_string(), cursor_style));
            if local + 1 < span_len {
                result.push(Span::styled(span.content[local + 1..].to_string(), span.style));
            }
        }
        pos += span_len;
    }
    result
}

/// Convert config::Color to ratatui Color.
pub(super) fn config_color_to_ratatui(c: config::Color) -> Color {
    match c {
        config::Color::Black => Color::Black,
        config::Color::Red => Color::Red,
        config::Color::Green => Color::Green,
        config::Color::Yellow => Color::Yellow,
        config::Color::Blue => Color::Blue,
        config::Color::Magenta => Color::Magenta,
        config::Color::Cyan => Color::Cyan,
        config::Color::White => Color::White,
        config::Color::DarkGray => Color::DarkGray,
        config::Color::LightRed => Color::LightRed,
        config::Color::LightGreen => Color::LightGreen,
        config::Color::LightYellow => Color::LightYellow,
        config::Color::LightBlue => Color::LightBlue,
        config::Color::LightMagenta => Color::LightMagenta,
        config::Color::LightCyan => Color::LightCyan,
        config::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        config::Color::Indexed(idx) => Color::Indexed(idx),
    }
}

pub(super) fn cell_color_to_ratatui(color: CellColor) -> Option<Color> {
    match color {
        CellColor::Default => None,
        CellColor::Black => Some(Color::Black),
        CellColor::Red => Some(Color::Red),
        CellColor::Green => Some(Color::Green),
        CellColor::Yellow => Some(Color::Yellow),
        CellColor::Blue => Some(Color::Blue),
        CellColor::Magenta => Some(Color::Magenta),
        CellColor::Cyan => Some(Color::Cyan),
        CellColor::White => Some(Color::White),
        CellColor::BrightBlack => Some(Color::DarkGray),
        CellColor::BrightRed => Some(Color::LightRed),
        CellColor::BrightGreen => Some(Color::LightGreen),
        CellColor::BrightYellow => Some(Color::LightYellow),
        CellColor::BrightBlue => Some(Color::LightBlue),
        CellColor::BrightMagenta => Some(Color::LightMagenta),
        CellColor::BrightCyan => Some(Color::LightCyan),
        CellColor::BrightWhite => Some(Color::White),
        CellColor::Indexed(idx) => Some(Color::Indexed(idx)),
        CellColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Convert grid cells to a styled ratatui Line (for colored terminal rendering).
pub(super) fn cells_to_styled_line(cells: &[novim_core::emulator::grid::Cell]) -> Line<'static> {
    // Merge consecutive cells with the same style into single spans
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let mut first = true;

    for cell in cells {
        let style = cell_to_style(cell.fg, cell.bg, cell.attrs);

        if first || style != current_style {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            current_style = style;
            first = false;
        }
        current_text.push(cell.c);
    }

    // Flush remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    Line::from(spans)
}

/// Convert grid CellColor + CellAttrs to a ratatui Style.
pub(super) fn cell_to_style(fg: CellColor, bg: CellColor, attrs: CellAttrs) -> Style {
    let mut style = Style::default();

    let fg_color = cell_color_to_ratatui(fg);
    let bg_color = cell_color_to_ratatui(bg);

    if let Some(c) = fg_color {
        style = style.fg(c);
    }
    if let Some(c) = bg_color {
        style = style.bg(c);
    }
    if attrs.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if attrs.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    if attrs.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if attrs.reverse {
        style = style.add_modifier(Modifier::REVERSED);
    }

    style
}
