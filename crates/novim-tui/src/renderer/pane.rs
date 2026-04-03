//! Single-pane rendering: viewport adjustment, cursor positioning, and pane content.

use novim_core::config;
use novim_core::text_utils::{expand_tabs, display_col};
use novim_core::pane::Pane;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::LineNumberMode;

use super::styling::{
    apply_highlights, styled_line_with_selection, apply_diagnostic_highlights,
    apply_search_highlights, apply_cursor_highlight, cells_to_styled_line,
};
use super::util::{wrapped_row_count, wrap_line};
use super::EditorState;

/// Adjust viewport_offset so the cursor stays visible.
pub(super) fn adjust_viewport(
    pane: &mut Pane,
    available_height: usize,
    text_width: usize,
    tab_width: usize,
    word_wrap: bool,
) {
    let content = pane.content.as_buffer_like();
    let cursor = content.cursor();

    if word_wrap && text_width > 0 {
        let mut rows_before_cursor = 0usize;
        for line in pane.viewport_offset..cursor.line {
            let raw = content.get_line(line).unwrap_or_default();
            let expanded = expand_tabs(&raw, tab_width);
            rows_before_cursor += wrapped_row_count(&expanded, text_width);
        }
        let cursor_raw = content.get_line(cursor.line).unwrap_or_default();
        let cursor_display = display_col(&cursor_raw, cursor.column, tab_width);
        let cursor_wrap_row = if text_width > 0 { cursor_display / text_width } else { 0 };
        let total_rows_to_cursor = rows_before_cursor + cursor_wrap_row;

        if cursor.line < pane.viewport_offset {
            pane.viewport_offset = cursor.line;
        } else if available_height > 0 && total_rows_to_cursor >= available_height {
            let mut offset = pane.viewport_offset;
            loop {
                let raw = content.get_line(offset).unwrap_or_default();
                let expanded = expand_tabs(&raw, tab_width);
                let rows = wrapped_row_count(&expanded, text_width);
                if rows_before_cursor < rows { break; }
                rows_before_cursor -= rows;
                offset += 1;
                if offset >= cursor.line { break; }
                let new_total = rows_before_cursor + cursor_wrap_row;
                if new_total < available_height { break; }
            }
            pane.viewport_offset = offset;
        }
    } else {
        if cursor.line < pane.viewport_offset {
            pane.viewport_offset = cursor.line;
        } else if available_height > 0 && cursor.line >= pane.viewport_offset + available_height {
            pane.viewport_offset = cursor.line.saturating_sub(available_height - 1);
        }
    }
}

/// Position the terminal cursor within a pane.
pub(super) fn position_cursor(
    f: &mut ratatui::Frame,
    pane: &Pane,
    area: Rect,
    ln_mode: LineNumberMode,
    tab_width: usize,
    borderless: bool,
    cursor_screen_row: Option<usize>,
    cursor_screen_col: Option<usize>,
    available_height: usize,
) {
    let content = pane.content.as_buffer_like();
    let cursor = content.cursor();
    let is_terminal = content.is_terminal();
    let border_off: u16 = if borderless { 0 } else { 1 };
    let col_offset: u16 = if is_terminal || ln_mode == LineNumberMode::Off {
        border_off
    } else {
        border_off + 5
    };

    if let (Some(row), Some(col)) = (cursor_screen_row, cursor_screen_col) {
        if row < available_height {
            f.set_cursor_position((
                area.x + col_offset + col as u16,
                area.y + border_off + row as u16,
            ));
        }
    } else {
        let cursor_line_on_screen = if is_terminal {
            cursor.line
        } else {
            cursor.line.saturating_sub(pane.viewport_offset)
        };
        if cursor_line_on_screen < available_height {
            let visual_col = if is_terminal {
                cursor.column
            } else {
                let raw = content.get_line(cursor.line).unwrap_or_default();
                display_col(&raw, cursor.column, tab_width)
            };
            f.set_cursor_position((
                area.x + col_offset + visual_col as u16,
                area.y + border_off + cursor_line_on_screen as u16,
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_single_pane(
    f: &mut ratatui::Frame,
    area: Rect,
    pane: &mut Pane,
    is_focused: bool,
    ln_mode: LineNumberMode,
    border_color: Color,
    diagnostics: Option<&Vec<novim_core::lsp::Diagnostic>>,
    search_pattern: Option<&str>,
    syntax_theme: &config::SyntaxTheme,
    tab_width: usize,
    word_wrap: bool,
    borderless: bool,
) {
    let content = pane.content.as_buffer_like();
    let border_overhead: u16 = if borderless { 0 } else { 2 };
    let available_height = area.height.saturating_sub(border_overhead) as usize;
    let is_terminal = content.is_terminal();
    let selection = content.selection();

    // Gutter width for text area calculation
    let gutter_width: usize = if is_terminal || ln_mode == LineNumberMode::Off { 0 } else { 5 };
    let text_width = (area.width.saturating_sub(border_overhead) as usize).saturating_sub(gutter_width);

    if !is_terminal {
        adjust_viewport(pane, available_height, text_width, tab_width, word_wrap);
    }

    let content = pane.content.as_buffer_like();
    let cursor = content.cursor();
    let total_lines = content.len_lines().max(1);
    let mut lines = Vec::with_capacity(available_height);
    let offset = if is_terminal { 0 } else { pane.viewport_offset };

    let secondary_cursors = content.secondary_cursors();
    let fold_state = content.fold_state();

    // Track which screen row the cursor lands on (for wrap-aware cursor positioning)
    let mut cursor_screen_row: Option<usize> = None;
    let mut cursor_screen_col: Option<usize> = None;
    let mut screen_row = 0usize;
    let mut line_num = offset;

    while screen_row < available_height && line_num < total_lines {
        // Skip lines hidden by collapsed folds
        if let Some(fs) = fold_state {
            if fs.is_line_hidden(line_num) {
                line_num += 1;
                continue;
            }
        }

        let raw_line = content.get_line(line_num).unwrap_or_default();
        let line_content = expand_tabs(&raw_line, tab_width).into_owned();

        // Show fold indicator if this line starts a collapsed fold
        let fold_indicator = fold_state.and_then(|fs| {
            fs.fold_at(line_num).and_then(|r| {
                if r.collapsed {
                    let hidden = r.end_line - r.start_line;
                    Some(format!(" ··· {} lines ···", hidden))
                } else {
                    None
                }
            })
        });

        if is_terminal {
            if let Some(cells) = content.get_styled_cells(line_num) {
                lines.push(cells_to_styled_line(cells));
            } else {
                lines.push(Line::from(Span::raw(line_content)));
            }
            screen_row += 1;
            line_num += 1;
        } else {
            let is_cursor_line = line_num == cursor.line;

            let diag_marker = diagnostics.and_then(|diags| {
                let has_error = diags.iter().any(|d| {
                    d.line == line_num && d.severity == novim_core::lsp::DiagnosticSeverity::Error
                });
                let has_warning = diags.iter().any(|d| {
                    d.line == line_num && d.severity == novim_core::lsp::DiagnosticSeverity::Warning
                });
                if has_error {
                    Some(Span::styled("E", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
                } else if has_warning {
                    Some(Span::styled("W", Style::default().fg(Color::Yellow)))
                } else {
                    None
                }
            });

            let num_style = if is_cursor_line {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let line_label = match ln_mode {
                LineNumberMode::Off => String::new(),
                LineNumberMode::Absolute => format!("{:>4} ", line_num + 1),
                LineNumberMode::Relative => {
                    let dist = (line_num as isize - cursor.line as isize).unsigned_abs();
                    format!("{:>4} ", dist)
                }
                LineNumberMode::Hybrid => {
                    if is_cursor_line {
                        format!("{:>4} ", line_num + 1)
                    } else {
                        let dist = (line_num as isize - cursor.line as isize).unsigned_abs();
                        format!("{:>4} ", dist)
                    }
                }
            };

            let mut text_spans = if let Some(sel) = selection {
                styled_line_with_selection(&line_content, line_num, sel)
            } else if let Some(hl_spans) = content.get_highlights(line_num) {
                apply_highlights(&line_content, hl_spans, syntax_theme)
            } else {
                vec![Span::raw(line_content.to_string())]
            };

            if let Some(diags) = diagnostics {
                let line_diags: Vec<_> = diags.iter().filter(|d| d.line == line_num).collect();
                if !line_diags.is_empty() {
                    text_spans = apply_diagnostic_highlights(&line_content, &text_spans, &line_diags);
                }
            }

            if let Some(pattern) = search_pattern {
                if !pattern.is_empty() {
                    text_spans = apply_search_highlights(&line_content, &text_spans, pattern);
                }
            }

            // Layer 3: Fold indicator
            if let Some(ref indicator) = fold_indicator {
                text_spans.push(Span::styled(
                    indicator.clone(),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
            }

            // Layer 4: Secondary cursor highlights
            for sc in secondary_cursors {
                if sc.line == line_num {
                    let sc_display = display_col(&raw_line, sc.column, tab_width);
                    text_spans = apply_cursor_highlight(&line_content, &text_spans, sc_display);
                }
            }

            // With word wrap, split the line into multiple screen rows
            let wrap_segments = if word_wrap && text_width > 0 && line_content.len() > text_width {
                wrap_line(&line_content, text_width)
            } else {
                vec![line_content.as_str()]
            };

            for (wrap_idx, _segment) in wrap_segments.iter().enumerate() {
                if screen_row >= available_height { break; }

                // Track cursor position on wrapped lines
                if is_cursor_line {
                    let cursor_display = display_col(&raw_line, cursor.column, tab_width);
                    let cursor_wrap = if text_width > 0 && word_wrap { cursor_display / text_width } else { 0 };
                    if wrap_idx == cursor_wrap {
                        cursor_screen_row = Some(screen_row);
                        cursor_screen_col = Some(if word_wrap && text_width > 0 { cursor_display % text_width } else { cursor_display });
                    }
                }

                if wrap_idx == 0 {
                    // First row: show gutter + full styled text (ratatui handles overflow)
                    let mut spans = Vec::new();
                    if !line_label.is_empty() {
                        if let Some(ref marker) = diag_marker {
                            let label = &line_label[..line_label.len().saturating_sub(1)];
                            spans.push(Span::styled(label.to_string(), num_style));
                            spans.push(marker.clone());
                        } else {
                            spans.push(Span::styled(line_label.clone(), num_style));
                        }
                    } else if let Some(ref marker) = diag_marker {
                        spans.push(marker.clone());
                    }
                    spans.extend(text_spans.clone());
                    lines.push(Line::from(spans));
                } else {
                    // Continuation rows: blank gutter + wrapped segment
                    let mut spans = Vec::new();
                    if !line_label.is_empty() {
                        spans.push(Span::styled("     ".to_string(), num_style));
                    }
                    let seg_start = wrap_idx * text_width;
                    let seg_end = ((wrap_idx + 1) * text_width).min(line_content.len());
                    spans.push(Span::raw(line_content[seg_start..seg_end].to_string()));
                    lines.push(Line::from(spans));
                }
                screen_row += 1;
            }
            line_num += 1;
        }
    }

    // Fill remaining rows with tilde
    while screen_row < available_height && !is_terminal {
        lines.push(Line::from(Span::styled(
            "   ~ ",
            Style::default().fg(Color::Blue),
        )));
        screen_row += 1;
    }

    if borderless {
        let paragraph = Paragraph::new(lines);
        f.render_widget(paragraph, area);
    } else {
        let title = {
            let name = pane.content.as_buffer_like().display_name();
            let dirty = if pane.content.as_buffer_like().is_dirty() { " [+]" } else { "" };
            format!(" {}{} ", name, dirty)
        };

        let border_style = Style::default().fg(border_color);

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );
        f.render_widget(paragraph, area);
    }

    if is_focused && available_height > 0 {
        position_cursor(f, pane, area, ln_mode, tab_width, borderless, cursor_screen_row, cursor_screen_col, available_height);
    }
}

/// Render the file explorer sidebar.
pub(super) fn render_explorer(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let Some(explorer) = &state.tabs[state.active_tab].explorer else { return };

    let available_height = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(available_height);

    // Scroll to keep cursor visible
    let scroll_offset = if explorer.cursor >= available_height {
        explorer.cursor - available_height + 1
    } else {
        0
    };

    for i in 0..available_height {
        let idx = scroll_offset + i;
        if let Some((name, is_dir, expanded, depth)) = explorer.entry_display(idx) {
            let is_cursor = idx == explorer.cursor;
            let indent = "  ".repeat(depth);
            let icon = if is_dir {
                if expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };

            let text = format!("{}{}{}", indent, icon, name);
            let style = if is_cursor && state.tabs[state.active_tab].explorer_focused {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if is_dir {
                Style::default().fg(Color::Indexed(75)) // soft blue
            } else {
                Style::default().fg(Color::Indexed(252)) // light gray
            };

            lines.push(Line::from(Span::styled(text, style)));
        }
    }

    let border_color = if state.tabs[state.active_tab].explorer_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Explorer ")
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(widget, area);
}
