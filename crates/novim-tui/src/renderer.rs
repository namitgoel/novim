//! Ratatui-based rendering for the editor UI.

use novim_core::config;
use novim_core::emulator::grid::{CellAttrs, CellColor};
use novim_core::highlight::HighlightGroup;
use novim_types::EditorMode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use novim_core::pane::Pane;

use super::EditorState;

/// Expand tab characters to spaces based on tab width.
fn expand_tabs(line: &str, tab_width: usize) -> String {
    if !line.contains('\t') {
        return line.to_string();
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
    result
}

/// Calculate display column accounting for tab expansion.
fn display_col(line: &str, cursor_col: usize, tab_width: usize) -> usize {
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

/// How many screen rows a line occupies when wrapped.
fn wrapped_row_count(line: &str, width: usize) -> usize {
    if width == 0 || line.is_empty() { return 1; }
    let len = line.len();
    ((len + width - 1) / width).max(1)
}

/// Split a line into wrapped segments of at most `width` characters.
fn wrap_line(line: &str, width: usize) -> Vec<&str> {
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

/// Highlight a single character at a display column for secondary cursors.
fn apply_cursor_highlight<'a>(line: &str, spans: &[Span<'a>], display_col: usize) -> Vec<Span<'a>> {
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

/// Render the full editor UI: panes + status bar (+ command line if active).
pub fn render(f: &mut ratatui::Frame, state: &mut EditorState) {
    let size = f.area();
    f.render_widget(Clear, size);

    let in_command_mode = state.mode == EditorMode::Command;
    let in_search_mode = state.search.active;
    let has_tabs = state.tabs.len() > 1;

    let mut constraints = Vec::new();
    if has_tabs {
        constraints.push(Constraint::Length(1)); // Tab bar
    }
    constraints.push(Constraint::Min(1)); // Main area
    constraints.push(Constraint::Length(1)); // Status bar
    if in_command_mode || in_search_mode {
        constraints.push(Constraint::Length(1)); // Command/search line
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(size);

    let mut chunk_idx = 0;

    // Render tab bar if multiple tabs
    if has_tabs {
        render_tab_bar(f, chunks[chunk_idx], state);
        chunk_idx += 1;
    }

    let main_chunk = chunks[chunk_idx];
    chunk_idx += 1;
    let status_chunk = chunks[chunk_idx];
    chunk_idx += 1;

    // Split horizontally if explorer is open
    let ws = &state.tabs[state.active_tab];
    let pane_area = if ws.explorer.is_some() {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(30), // explorer sidebar
                Constraint::Min(1),     // panes
            ])
            .split(main_chunk);
        render_explorer(f, hsplit[0], state);
        hsplit[1]
    } else {
        main_chunk
    };

    render_panes(f, pane_area, state);
    render_status_bar(f, status_chunk, state);

    if in_command_mode || in_search_mode {
        let cmd_chunk = chunks[chunk_idx];
        if in_command_mode {
            render_command_line(f, cmd_chunk, state);
        } else {
            render_search_line(f, cmd_chunk, state);
        }
    }

    // Popups render on top of everything
    if state.show_help {
        render_help_popup(f, size, state.help_scroll);
    } else if state.tabs[state.active_tab].show_buffer_list {
        render_buffer_list(f, size, state);
    } else if state.show_workspace_list {
        render_workspace_list(f, size, state);
    }

    // Completion popup (near cursor)
    if state.completion.visible && !state.completion.items.is_empty() {
        render_completion_popup(f, size, state);
    }

    // File finder popup
    if state.finder.visible {
        render_file_finder(f, size, state);
    }

    // Hover info popup
    if let Some(hover_text) = &state.hover_text {
        render_hover_popup(f, size, hover_text, state);
    }
}

/// Render the tab bar at the top of the screen.
/// Tab colors — each workspace gets a unique accent color.
const TAB_COLORS: &[u8] = &[
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

fn render_tab_bar(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let mut spans = Vec::new();
    for (i, ws) in state.tabs.iter().enumerate() {
        let is_active = i == state.active_tab;
        let accent = Color::Indexed(TAB_COLORS[i % TAB_COLORS.len()]);
        let style = if is_active {
            Style::default().bg(accent).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(accent)
        };
        spans.push(Span::styled(format!(" {} ", ws.name), style));
        if i + 1 < state.tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(Color::Indexed(240))));
        }
    }
    let line = Line::from(spans);
    let widget = Paragraph::new(line).style(Style::default().bg(Color::Indexed(236)));
    f.render_widget(widget, area);
}

fn render_panes(f: &mut ratatui::Frame, area: Rect, state: &mut EditorState) {
    let idx = state.active_tab;
    let focused_id = state.tabs[idx].panes.focused_id();
    let ln_mode = state.line_number_mode;
    let focused_color = config_color_to_ratatui(config::parse_color(&state.config.theme.focused_border));
    let unfocused_color = config_color_to_ratatui(config::parse_color(&state.config.theme.unfocused_border));
    let syntax_theme = state.config.syntax_theme.clone();
    let tab_width = state.config.editor.tab_width;
    let word_wrap = state.config.editor.word_wrap;
    let search_pattern = state.search.pattern.clone();
    let core_area = novim_types::Rect::new(area.x, area.y, area.width, area.height);
    let layouts = state.tabs[idx].panes.layout(core_area);

    let ws = &mut state.tabs[idx];
    for (pane_id, rect) in &layouts {
        let is_focused = *pane_id == focused_id;
        let ratatui_rect = Rect::new(rect.x, rect.y, rect.width, rect.height);
        // Look up diagnostics URI before borrowing pane mutably
        let diag_uri = ws.panes.get_pane(*pane_id).and_then(|pane| {
            match &pane.content {
                novim_core::pane::PaneContent::Editor(buf) => buf.file_uri(),
                _ => None,
            }
        });
        let diags = diag_uri.and_then(|uri| ws.diagnostics.get(&uri));
        if let Some(pane) = ws.panes.get_pane_mut(*pane_id) {
            let border_color = if is_focused { focused_color } else { unfocused_color };
            render_single_pane(f, ratatui_rect, pane, is_focused, ln_mode, border_color, diags, search_pattern.as_deref(), &syntax_theme, tab_width, word_wrap);
        }
    }
}

fn render_single_pane(
    f: &mut ratatui::Frame,
    area: Rect,
    pane: &mut Pane,
    is_focused: bool,
    ln_mode: super::LineNumberMode,
    border_color: Color,
    diagnostics: Option<&Vec<novim_core::lsp::Diagnostic>>,
    search_pattern: Option<&str>,
    syntax_theme: &config::SyntaxTheme,
    tab_width: usize,
    word_wrap: bool,
) {
    let content = pane.content.as_buffer_like();
    let available_height = area.height.saturating_sub(2) as usize;
    let cursor = content.cursor();
    let is_terminal = content.is_terminal();
    let selection = content.selection();

    // Gutter width for text area calculation
    let gutter_width: usize = if is_terminal || ln_mode == super::LineNumberMode::Off { 0 } else { 5 };
    let text_width = (area.width.saturating_sub(2) as usize).saturating_sub(gutter_width); // minus borders

    if !is_terminal {
        if word_wrap && text_width > 0 {
            // With wrap, we need to count wrapped rows to keep cursor visible
            let mut rows_before_cursor = 0usize;
            for line in pane.viewport_offset..cursor.line {
                let raw = content.get_line(line).unwrap_or_default();
                let expanded = expand_tabs(&raw, tab_width);
                rows_before_cursor += wrapped_row_count(&expanded, text_width);
            }
            // Cursor's wrapped row within its line
            let cursor_raw = content.get_line(cursor.line).unwrap_or_default();
            let cursor_display = display_col(&cursor_raw, cursor.column, tab_width);
            let cursor_wrap_row = if text_width > 0 { cursor_display / text_width } else { 0 };
            let total_rows_to_cursor = rows_before_cursor + cursor_wrap_row;

            if cursor.line < pane.viewport_offset {
                pane.viewport_offset = cursor.line;
            } else if available_height > 0 && total_rows_to_cursor >= available_height {
                // Scroll up until cursor fits
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

    let content = pane.content.as_buffer_like();
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
        let line_content = expand_tabs(&raw_line, tab_width);

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
                super::LineNumberMode::Off => String::new(),
                super::LineNumberMode::Absolute => format!("{:>4} ", line_num + 1),
                super::LineNumberMode::Relative => {
                    let dist = (line_num as isize - cursor.line as isize).unsigned_abs();
                    format!("{:>4} ", dist)
                }
                super::LineNumberMode::Hybrid => {
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

    if is_focused && available_height > 0 {
        let cursor = pane.content.as_buffer_like().cursor();
        let col_offset: u16 = if is_terminal {
            1
        } else if ln_mode == super::LineNumberMode::Off {
            1
        } else {
            6
        };

        // Use wrap-aware cursor position if available
        if let (Some(row), Some(col)) = (cursor_screen_row, cursor_screen_col) {
            if row < available_height {
                f.set_cursor_position((
                    area.x + col_offset + col as u16,
                    area.y + 1 + row as u16,
                ));
            }
        } else {
            let cursor_line_on_screen = if is_terminal {
                cursor.line
            } else if cursor.line >= pane.viewport_offset {
                cursor.line - pane.viewport_offset
            } else {
                0
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
                    area.y + 1 + cursor_line_on_screen as u16,
                ));
            }
        }
    }
}

fn render_status_bar(f: &mut ratatui::Frame, area: Rect, state: &mut EditorState) {
    let idx = state.active_tab;
    let pane_count = state.tabs[idx].panes.pane_count();

    // Count diagnostics for the focused file
    let diag_info = {
        let pane = state.tabs[idx].panes.focused_pane();
        let uri = match &pane.content {
            novim_core::pane::PaneContent::Editor(buf) => buf.file_uri(),
            _ => None,
        };
        if let Some(diags) = uri.and_then(|u| state.tabs[idx].diagnostics.get(&u)) {
            let errors = diags.iter().filter(|d| d.severity == novim_core::lsp::DiagnosticSeverity::Error).count();
            let warnings = diags.iter().filter(|d| d.severity == novim_core::lsp::DiagnosticSeverity::Warning).count();
            if errors > 0 || warnings > 0 {
                format!(" {}E {}W", errors, warnings)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    let pane = state.tabs[idx].panes.focused_pane_mut();
    let cursor = pane.content.as_buffer_like().cursor();
    let total = pane.content.as_buffer_like().len_lines();
    let is_terminal = pane.content.as_buffer_like().is_terminal();

    // LSP status indicator with progress
    let lsp_status = if !state.tabs[idx].lsp_clients.is_empty() {
        let langs: Vec<&str> = state.tabs[idx].lsp_clients.keys().map(|s| s.as_str()).collect();
        if let Some(status) = &state.tabs[idx].lsp_status {
            format!(" LSP:{}[{}]", langs.join(","), status)
        } else {
            format!(" LSP:{}", langs.join(","))
        }
    } else {
        String::new()
    };

    let right = format!(
        "{} | {}:{} | {}/{} ",
        lsp_status,
        cursor.line + 1,
        cursor.column + 1,
        cursor.line + 1,
        total,
    );

    let pane_info = if pane_count > 1 {
        format!(" [pane {}/{}]", state.tabs[idx].panes.focused_id() + 1, pane_count)
    } else {
        String::new()
    };

    let mode_name = if state.macros.recording.is_some() {
        let reg = state.macros.recording.unwrap();
        &format!("REC @{}", reg)
    } else if state.input_state == novim_core::input::InputState::WaitingPaneCommand {
        "CTRL+W..."
    } else if is_terminal {
        "TERMINAL"
    } else {
        state.mode.display_name()
    };

    let left = if let Some(msg) = state.status_message.take() {
        format!(" {} | {}{}{}", mode_name, msg, diag_info, pane_info)
    } else {
        format!(" {}{}{}", mode_name, diag_info, pane_info)
    };

    let padding = (area.width as usize).saturating_sub(left.len() + right.len());
    let text = format!("{}{:padding$}{}", left, "", right, padding = padding);

    let widget =
        Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(widget, area);
}

fn render_command_line(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let text = format!(":{}", state.command_buffer);
    let widget = Paragraph::new(Line::from(vec![
        Span::styled(":", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(&state.command_buffer),
    ]));
    f.render_widget(widget, area);

    f.set_cursor_position((
        area.x + text.len() as u16,
        area.y,
    ));
}

/// Render the file explorer sidebar.
fn render_explorer(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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

/// Render the search input line.
fn render_search_line(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let text = format!("/{}", state.search.buffer);
    let widget = Paragraph::new(Line::from(vec![
        Span::styled("/", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(&state.search.buffer),
    ]));
    f.render_widget(widget, area);

    f.set_cursor_position((
        area.x + text.len() as u16,
        area.y,
    ));
}

/// Render centered help popup overlay.
fn render_help_popup(f: &mut ratatui::Frame, area: Rect, scroll: usize) {
    let popup_width = 56u16.min(area.width.saturating_sub(4));
    let popup_height = (area.height * 4 / 5).max(20).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let h = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let k = Style::default().fg(Color::Yellow);
    let d = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);

    let all_lines = vec![
        Line::from(Span::styled("  Keyboard Shortcuts", h)),
        Line::from(""),
        Line::from(Span::styled("  Navigation", h)),
        help_line("  h/j/k/l", "Move cursor", k, d),
        help_line("  Ctrl+U / Ctrl+D", "Scroll up/down", k, d),
        help_line("  5j / 3k", "Move N lines", k, d),
        help_line("  /pattern", "Search (regex)", k, d),
        help_line("  n / N", "Next / prev match", k, d),
        help_line("  Esc", "Clear search / Normal mode", k, d),
        Line::from(""),
        Line::from(Span::styled("  Editing", h)),
        help_line("  i", "Insert mode", k, d),
        help_line("  v", "Visual mode (select)", k, d),
        help_line("  u / Ctrl+R", "Undo / Redo", k, d),
        help_line("  p", "Paste", k, d),
        help_line("  dd / 3dd", "Delete line(s)", k, d),
        help_line("  cc", "Change line", k, d),
        help_line("  dl / dh", "Delete char right/left", k, d),
        help_line("  :", "Command mode", k, d),
        Line::from(""),
        Line::from(Span::styled("  File", h)),
        help_line("  Ctrl+S / :w", "Save", k, d),
        help_line("  :q / :q! / :wq", "Quit / Force / Save+quit", k, d),
        help_line("  :e <file>", "Open file", k, d),
        help_line("  Ctrl+F", "File finder", k, d),
        help_line("  :explore", "File explorer", k, d),
        help_line("  :ls", "Buffer list", k, d),
        help_line("  :bn / :bp", "Next / prev buffer", k, d),
        Line::from(""),
        Line::from(Span::styled("  Panes (Ctrl+W prefix)", h)),
        help_line("  Ctrl+W v", "Vertical split", k, d),
        help_line("  Ctrl+W s", "Horizontal split", k, d),
        help_line("  Ctrl+W h/j/k/l", "Move focus", k, d),
        help_line("  Ctrl+W q", "Close pane", k, d),
        help_line("  Ctrl+W t", "Open terminal", k, d),
        help_line("  Ctrl+W f", "File finder", k, d),
        help_line("  Ctrl+W e", "Focus explorer", k, d),
        help_line("  Ctrl+W :", "Command mode (terminal)", k, d),
        Line::from(""),
        Line::from(Span::styled("  Workspaces", h)),
        help_line("  :tabnew <path>", "New workspace", k, d),
        help_line("  gt / gT", "Next / prev workspace", k, d),
        help_line("  Ctrl+W n / N", "Next / prev workspace", k, d),
        help_line("  Ctrl+W L", "List workspaces", k, d),
        help_line("  Ctrl+W 1-9", "Jump to workspace", k, d),
        help_line("  :tabclose", "Close workspace", k, d),
        help_line("  :tabrename <n>", "Rename workspace", k, d),
        Line::from(""),
        Line::from(Span::styled("  Folding", h)),
        help_line("  za", "Toggle fold at cursor", k, d),
        help_line("  zM", "Fold all", k, d),
        help_line("  zR", "Unfold all", k, d),
        Line::from(""),
        Line::from(Span::styled("  Multi-Cursor", h)),
        help_line("  Alt+Up", "Add cursor above", k, d),
        help_line("  Alt+Down", "Add cursor below", k, d),
        help_line("  Esc", "Clear extra cursors", k, d),
        Line::from(""),
        Line::from(Span::styled("  LSP", h)),
        help_line("  gd", "Go to definition", k, d),
        help_line("  K (shift)", "Hover info", k, d),
        help_line("  Ctrl+N", "Autocomplete", k, d),
        Line::from(""),
        Line::from(Span::styled("  Macros", h)),
        help_line("  Qa", "Start recording @a", k, d),
        help_line("  Qa (again)", "Stop recording", k, d),
        help_line("  @a", "Replay macro @a", k, d),
        help_line("  @@", "Replay last macro", k, d),
        Line::from(""),
        Line::from(Span::styled("  Other", h)),
        help_line("  :%s/old/new", "Replace all (regex)", k, d),
        help_line("  :mksession", "Save session", k, d),
        help_line("  :set rnu/nonu", "Line number mode", k, d),
        help_line("  :set wrap/nowrap", "Toggle word wrap", k, d),
        help_line("  :set et/noet", "Expand tab on/off", k, d),
        help_line("  :set ai/noai", "Auto-indent on/off", k, d),
        help_line("  :set ts=N", "Set tab width", k, d),
        help_line("  Ctrl+L", "Redraw screen", k, d),
        Line::from(""),
        Line::from(Span::styled("  ↑/↓ scroll | Esc close", dim)),
    ];

    let visible_height = popup_height.saturating_sub(2) as usize;
    let max_scroll = all_lines.len().saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);
    let visible_lines: Vec<Line> = all_lines.into_iter().skip(scroll).take(visible_height).collect();

    let title = format!(" Help ({}/{}) ", scroll + 1, max_scroll + 1);
    let popup = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );

    f.render_widget(popup, popup_area);
}

fn help_line<'a>(key: &'a str, desc: &'a str, key_style: Style, desc_style: Style) -> Line<'a> {
    let padding = 20usize.saturating_sub(key.len());
    Line::from(vec![
        Span::styled(key, key_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(desc, desc_style),
    ])
}

/// Render buffer list popup.
fn render_buffer_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let ws = &state.tabs[state.active_tab];
    let history = &ws.buffer_history;
    let current_idx = ws.buffer_history_idx;

    let popup_height = (history.len() as u16 + 4).min(area.height.saturating_sub(4));
    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let mut lines = vec![
        Line::from(Span::styled(
            "  Open Buffers",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if history.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no files open)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, path) in history.iter().enumerate() {
            let marker = if i == current_idx { ">" } else { " " };
            let style = if i == current_idx {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", marker, path),
                style,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  :bn/:bp to switch | Esc to close",
        Style::default().fg(Color::DarkGray),
    )));

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Buffers ")
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );
    f.render_widget(popup, popup_area);
}

/// Render completion popup near the cursor.
fn render_completion_popup(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let items = &state.completion.items;
    let selected = state.completion.selected;

    // Get cursor screen position (approximate)
    let cursor = state.tabs[state.active_tab].panes.focused_pane().content.as_buffer_like().cursor();
    let popup_width = 40u16.min(area.width.saturating_sub(10));
    let popup_height = (items.len() as u16 + 2).min(12).min(area.height.saturating_sub(4));

    // Position below cursor, offset by gutter
    let x = (6 + cursor.column as u16 + 1).min(area.width.saturating_sub(popup_width));
    let y = (cursor.line as u16 + 2).min(area.height.saturating_sub(popup_height));
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let visible_count = (popup_height.saturating_sub(2)) as usize;
    let scroll = if selected >= visible_count {
        selected - visible_count + 1
    } else {
        0
    };

    let mut lines = Vec::new();
    for (i, item) in items.iter().enumerate().skip(scroll).take(visible_count) {
        let is_selected = i == selected;
        let kind_icon = match item.kind {
            novim_core::lsp::CompletionKind::Function => "f",
            novim_core::lsp::CompletionKind::Variable => "v",
            novim_core::lsp::CompletionKind::Field => ".",
            novim_core::lsp::CompletionKind::Type => "T",
            novim_core::lsp::CompletionKind::Keyword => "k",
            novim_core::lsp::CompletionKind::Module => "M",
            novim_core::lsp::CompletionKind::Property => "p",
            novim_core::lsp::CompletionKind::Other => " ",
        };

        let label = if item.label.len() > (popup_width as usize - 6) {
            format!("{}...", &item.label[..popup_width as usize - 9])
        } else {
            item.label.clone()
        };

        let style = if is_selected {
            Style::default().bg(Color::Indexed(24)).fg(Color::White) // highlighted
        } else {
            Style::default().bg(Color::Indexed(236)).fg(Color::Indexed(252)) // dark bg
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", kind_icon), style.fg(Color::Indexed(75))),
            Span::styled(label, style),
        ]));
    }

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Indexed(240)))
            .style(Style::default().bg(Color::Indexed(236))),
    );
    f.render_widget(popup, popup_area);
}

/// Render the file finder popup (Ctrl+P).
fn render_file_finder(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let show_preview = state.config.editor.finder_preview && area.width > 80;

    let total_width = if show_preview {
        (area.width * 4 / 5).max(80).min(area.width.saturating_sub(4)) // 80% width
    } else {
        60u16.min(area.width.saturating_sub(10))
    };
    let popup_height = (area.height * 4 / 5).max(15).min(area.height.saturating_sub(2)); // 80% height
    let x = (area.width.saturating_sub(total_width)) / 2; // centered
    let y = (area.height.saturating_sub(popup_height)) / 2; // centered
    let popup_area = Rect::new(x, y, total_width, popup_height);

    f.render_widget(Clear, popup_area);

    if show_preview {
        // Split: left = file list (45%), right = preview (55%)
        let list_width = (total_width * 45 / 100).max(35);
        let preview_width = total_width.saturating_sub(list_width);

        let list_area = Rect::new(x, y, list_width, popup_height);
        let preview_area = Rect::new(x + list_width, y, preview_width, popup_height);

        render_finder_list(f, list_area, state);
        render_finder_preview(f, preview_area, state);
    } else {
        render_finder_list(f, popup_area, state);
    }
}

fn render_finder_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let visible_count = area.height.saturating_sub(4) as usize;
    let list_width = area.width;
    let mut lines = Vec::new();

    // Search input
    lines.push(Line::from(vec![
        Span::styled(" > ", Style::default().fg(Color::Cyan)),
        Span::styled(&state.finder.query, Style::default().fg(Color::White)),
        Span::styled("_", Style::default().fg(Color::DarkGray)),
    ]));

    // Separator
    lines.push(Line::from(Span::styled(
        "─".repeat(list_width.saturating_sub(2) as usize),
        Style::default().fg(Color::Indexed(240)),
    )));

    let scroll = if state.finder.selected >= visible_count {
        state.finder.selected - visible_count + 1
    } else {
        0
    };

    for (i, result) in state.finder.results.iter().enumerate().skip(scroll).take(visible_count) {
        let is_selected = i == state.finder.selected;
        let style = if is_selected {
            Style::default().bg(Color::Indexed(24)).fg(Color::White)
        } else {
            Style::default().fg(Color::Indexed(252))
        };

        let max_len = list_width as usize - 4;
        let display = if result.display.len() > max_len {
            format!("...{}", &result.display[result.display.len().saturating_sub(max_len - 3)..])
        } else {
            result.display.clone()
        };

        lines.push(Line::from(Span::styled(format!("  {}", display), style)));
    }

    if state.finder.results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No files found",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let title = format!(" Find Files ({}) ", state.finder.results.len());
    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Indexed(235))),
    );
    f.render_widget(popup, area);
}

fn render_finder_preview(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let available_height = area.height.saturating_sub(2) as usize;
    let syntax_theme = &state.config.syntax_theme;
    let mut lines = Vec::new();

    if state.finder.preview_lines.is_empty() {
        lines.push(Line::from(Span::styled(
            " (no preview)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, line) in state.finder.preview_lines.iter().enumerate().take(available_height) {
            let num = format!("{:>3} ", i + 1);
            let max_width = area.width as usize - 7;
            let content = if line.len() > max_width {
                &line[..max_width]
            } else {
                line.as_str()
            };

            // Apply syntax highlighting if available
            let text_spans = if let Some(hl_spans) = state.finder.preview_highlights.get(i) {
                if !hl_spans.is_empty() {
                    apply_highlights(content, hl_spans, syntax_theme)
                } else {
                    vec![Span::raw(content.to_string())]
                }
            } else {
                vec![Span::raw(content.to_string())]
            };

            let mut spans = vec![Span::styled(num, Style::default().fg(Color::Indexed(243)))];
            spans.extend(text_spans);
            lines.push(Line::from(spans));
        }
    }

    // Show filename in title
    let title = state.finder.results.get(state.finder.selected)
        .map(|r| {
            let name = r.display.rsplit('/').next().unwrap_or(&r.display);
            format!(" {} ", name)
        })
        .unwrap_or_else(|| " Preview ".to_string());

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Indexed(240)))
            .style(Style::default().bg(Color::Indexed(235))),
    );
    f.render_widget(popup, area);
}

/// Render hover info popup near the cursor.
fn render_hover_popup(f: &mut ratatui::Frame, area: Rect, text: &str, state: &EditorState) {
    let lines: Vec<&str> = text.lines().collect();
    let max_line_width = lines.iter().map(|l| l.len()).max().unwrap_or(20);

    let popup_width = (max_line_width as u16 + 4).min(area.width.saturating_sub(10)).max(20);
    let popup_height = (lines.len() as u16 + 2).min(15).min(area.height.saturating_sub(4));

    // Position above cursor
    let cursor = state.tabs[state.active_tab].panes.focused_pane().content.as_buffer_like().cursor();
    let x = (6 + cursor.column as u16).min(area.width.saturating_sub(popup_width));
    let y = if cursor.line as u16 > popup_height + 2 {
        cursor.line as u16 - popup_height
    } else {
        (cursor.line as u16 + 2).min(area.height.saturating_sub(popup_height))
    };
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let display_lines: Vec<Line> = lines
        .iter()
        .take((popup_height.saturating_sub(2)) as usize)
        .map(|l| {
            Line::from(Span::styled(
                format!(" {} ", l),
                Style::default().fg(Color::Indexed(252)),
            ))
        })
        .collect();

    let popup = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Hover ")
            .border_style(Style::default().fg(Color::Indexed(75)))
            .style(Style::default().bg(Color::Indexed(236))),
    );
    f.render_widget(popup, popup_area);
}

/// Render workspace list popup.
fn render_workspace_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let tabs = &state.tabs;

    // Auto-size: width based on longest entry, height based on tab count
    let max_entry_len = tabs.iter().enumerate().map(|(i, ws)| {
        format!("  * {}. {}  [{} pane(s)]  {}", i + 1, ws.name, ws.panes.pane_count(), ws.launch_dir.display()).len()
    }).max().unwrap_or(40);
    let popup_width = ((max_entry_len as u16) + 6).max(50).min(area.width.saturating_sub(4));
    let popup_height = (tabs.len() as u16 + 6).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let mut lines = vec![
        Line::from(Span::styled(
            "  Workspaces",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (i, ws) in tabs.iter().enumerate() {
        let is_active = i == state.active_tab;
        let is_selected = i == state.workspace_list_selected;
        let marker = if is_active { "*" } else { " " };
        let pane_info = format!("{} pane(s)", ws.panes.pane_count());
        let dir = ws.launch_dir.to_string_lossy();
        let style = if is_selected {
            Style::default().bg(Color::Indexed(24)).fg(Color::White)
        } else if is_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Indexed(252))
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}. {}  [{}]  {}", marker, i + 1, ws.name, pane_info, dir),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ select | Enter switch | Esc close",
        Style::default().fg(Color::DarkGray),
    )));

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Workspaces ")
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );
    f.render_widget(popup, popup_area);
}

/// Convert config::Color to ratatui Color.
fn config_color_to_ratatui(c: config::Color) -> Color {
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

/// Convert grid cells to a styled ratatui Line (for colored terminal rendering).
fn cells_to_styled_line(cells: &[novim_core::emulator::grid::Cell]) -> Line<'static> {
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
fn cell_to_style(fg: CellColor, bg: CellColor, attrs: CellAttrs) -> Style {
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

/// Apply syntax highlight spans to a line.
fn apply_highlights(content: &str, spans: &[novim_core::highlight::HighlightSpan], theme: &config::SyntaxTheme) -> Vec<Span<'static>> {
    if spans.is_empty() {
        return vec![Span::raw(content.to_string())];
    }

    let mut result = Vec::new();
    let mut pos = 0;

    for span in spans {
        let start = span.start.min(content.len());
        let end = span.end.min(content.len());

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

fn highlight_group_to_style(group: HighlightGroup, theme: &config::SyntaxTheme) -> Style {
    let color_str = match group {
        HighlightGroup::Keyword => &theme.keyword,
        HighlightGroup::Function | HighlightGroup::FunctionBuiltin => &theme.function,
        HighlightGroup::Type | HighlightGroup::TypeBuiltin => &theme.r#type,
        HighlightGroup::Variable | HighlightGroup::VariableBuiltin => &theme.variable,
        HighlightGroup::Constant | HighlightGroup::ConstantBuiltin => &theme.constant,
        HighlightGroup::String => &theme.string,
        HighlightGroup::Number => &theme.number,
        HighlightGroup::Comment => &theme.comment,
        HighlightGroup::Operator => &theme.operator,
        HighlightGroup::Punctuation | HighlightGroup::PunctuationBracket | HighlightGroup::PunctuationDelimiter => &theme.punctuation,
        HighlightGroup::Property => &theme.property,
        HighlightGroup::Attribute => &theme.attribute,
        HighlightGroup::Tag => &theme.property,
        HighlightGroup::Escape => &theme.constant,
        HighlightGroup::None => return Style::default(),
    };

    let color = config_color_to_ratatui(config::parse_color(color_str));
    let mut style = Style::default().fg(color);

    // Keywords get bold
    if matches!(group, HighlightGroup::Keyword) {
        style = style.add_modifier(Modifier::BOLD);
    }

    style
}

/// Render a line with selection highlighting.
fn styled_line_with_selection(
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
    let sel_start_col = if line_num == start.line { start.column } else { 0 };
    let sel_end_col = if line_num == end.line {
        (end.column + 1).min(line_len)
    } else {
        line_len
    };

    let sel_start_col = sel_start_col.min(line_len);
    let sel_end_col = sel_end_col.min(line_len);

    let mut spans = Vec::new();

    // Before selection
    if sel_start_col > 0 {
        spans.push(Span::raw(content[..sel_start_col].to_string()));
    }

    // Selected portion
    if sel_start_col < sel_end_col {
        spans.push(Span::styled(
            content[sel_start_col..sel_end_col].to_string(),
            sel_style,
        ));
    }

    // After selection
    if sel_end_col < line_len {
        spans.push(Span::raw(content[sel_end_col..].to_string()));
    }

    if spans.is_empty() {
        vec![Span::raw(content.to_string())]
    } else {
        spans
    }
}

/// Apply diagnostic underlines on top of existing styled spans.
/// Preserves original syntax colors, adds underline on diagnostic ranges.
fn apply_diagnostic_highlights<'a>(
    content: &str,
    existing_spans: &[Span<'a>],
    diags: &[&novim_core::lsp::Diagnostic],
) -> Vec<Span<'static>> {
    let mut ranges: Vec<(usize, usize, novim_core::lsp::DiagnosticSeverity)> = diags
        .iter()
        .map(|d| (d.col_start.min(content.len()), d.col_end.min(content.len()), d.severity))
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
fn apply_search_highlights<'a>(
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

fn cell_color_to_ratatui(color: CellColor) -> Option<Color> {
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
    }
}
