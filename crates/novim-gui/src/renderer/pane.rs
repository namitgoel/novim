//! Pane rendering: single pane, tab bar, status bar, command/search lines, explorer, welcome screen.

use glyphon::Color;
use novim_core::config::SyntaxTheme;
use novim_core::editor::{EditorState, LineNumberMode};
use novim_core::text_utils::{expand_tabs, display_col};
use novim_core::welcome;
use novim_types::EditorMode;

use super::theme::*;
use super::styling::*;

// ── Pane rendering ────────────────────────────────────────────────────────────

pub(super) fn render_pane_area(editor: &EditorState, cols: usize, rows: usize) -> Vec<Vec<RichSpan>> {
    let idx = editor.active_tab;
    let ws = &editor.tabs[idx];
    let focused_id = ws.panes.focused_id();
    let ln_mode = editor.line_number_mode;
    let syntax_theme = &editor.config.syntax_theme;
    let tab_width = editor.config.editor.tab_width;
    let search_pattern = if editor.search.active || editor.search.pattern.as_ref().is_some_and(|s| !s.is_empty()) {
        editor.search.pattern.as_deref()
    } else {
        None
    };

    let area = novim_types::Rect::new(0, 0, cols as u16, rows as u16);
    let layouts = ws.panes.layout(area);

    // For single-pane, render directly
    if layouts.len() == 1 {
        let (pane_id, _rect) = &layouts[0];
        let is_focused = *pane_id == focused_id;
        if let Some(pane) = ws.panes.get_pane(*pane_id) {
            return render_single_pane(pane, is_focused, ln_mode, syntax_theme, tab_width, search_pattern, cols, rows, ws, editor, false);
        }
    }

    // Multi-pane: render each pane into its own sub-grid, then composite
    let mut grid: Vec<Vec<RichSpan>> = (0..rows)
        .map(|_| vec![RichSpan { text: " ".repeat(cols), color: DIM }])
        .collect();

    for (pane_id, rect) in &layouts {
        let is_focused = *pane_id == focused_id;
        if let Some(pane) = ws.panes.get_pane(*pane_id) {
            let pane_w = rect.width as usize;
            let pane_h = rect.height as usize;
            let pane_lines = render_single_pane(pane, is_focused, ln_mode, syntax_theme, tab_width, search_pattern, pane_w, pane_h, ws, editor, true);

            for (row_offset, line_spans) in pane_lines.iter().enumerate() {
                let screen_row = rect.y as usize + row_offset;
                if screen_row < rows {
                    // Replace the grid row segment
                    // Build a full-width line with pane content inserted at rect.x
                    let before_cols = rect.x as usize;
                    let mut new_line = Vec::new();
                    if before_cols > 0 {
                        new_line.push(RichSpan { text: " ".repeat(before_cols), color: DIM });
                    }
                    new_line.extend(line_spans.iter().map(|s| RichSpan { text: s.text.clone(), color: s.color }));
                    // Pad to pane width
                    let used: usize = line_spans.iter().map(|s| s.text.chars().count()).sum();
                    if used < pane_w {
                        new_line.push(RichSpan { text: " ".repeat(pane_w - used), color: DIM });
                    }
                    grid[screen_row] = new_line;
                }
            }
        }
    }

    grid
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_single_pane(
    pane: &novim_core::pane::Pane,
    is_focused: bool,
    ln_mode: LineNumberMode,
    syntax_theme: &SyntaxTheme,
    tab_width: usize,
    search_pattern: Option<&str>,
    cols: usize,
    rows: usize,
    ws: &novim_core::editor::Workspace,
    _editor: &EditorState,
    draw_border: bool,
) -> Vec<Vec<RichSpan>> {
    let buf = pane.content.as_buffer_like();
    let is_terminal = buf.is_terminal();
    let cursor = buf.cursor();
    let selection = buf.selection();
    let total_lines = buf.len_lines().max(1);
    let offset = pane.viewport_offset;

    let gutter_width: usize = if is_terminal || ln_mode == LineNumberMode::Off { 0 } else { 5 };
    let text_cols = cols.saturating_sub(gutter_width);

    let border_color = if is_focused { BLUE } else { DIM };
    let border_offset: usize = if draw_border { 1 } else { 0 };

    let (content_rows, inner_cols) = if draw_border {
        (rows.saturating_sub(2), cols.saturating_sub(2))
    } else {
        (rows, cols)
    };

    let mut lines: Vec<Vec<RichSpan>> = Vec::with_capacity(rows);

    if draw_border {
        // Title bar
        let name = buf.display_name();
        let dirty = if buf.is_dirty() { " [+]" } else { "" };
        let title = format!(" {}{} ", name, dirty);
        let title_row = {
            let mut spans = Vec::new();
            let bar: String = "─".repeat(2);
            spans.push(RichSpan { text: "┌".to_string(), color: border_color });
            spans.push(RichSpan { text: bar, color: border_color });
            let title_display = truncate_str(&title, cols.saturating_sub(6));
            spans.push(RichSpan { text: title_display, color: if is_focused { YELLOW } else { FG } });
            let used: usize = spans.iter().map(|s| s.text.chars().count()).sum();
            if used < cols.saturating_sub(1) {
                spans.push(RichSpan { text: "─".repeat(cols.saturating_sub(used + 1)), color: border_color });
            }
            spans.push(RichSpan { text: "┐".to_string(), color: border_color });
            spans
        };
        lines.push(title_row);
    }

    let mut line_num = offset;
    for _screen_row in 0..content_rows {
        let mut row_spans = Vec::new();
        if draw_border {
            row_spans.push(RichSpan { text: "│".to_string(), color: border_color });
        }

        if line_num < total_lines {
            let is_cursor_line = line_num == cursor.line && is_focused;

            // Gutter (line numbers)
            if gutter_width > 0 && !is_terminal {
                let num_color = if is_cursor_line { LINE_NUM_ACTIVE } else { LINE_NUM };

                // Diagnostic marker
                let diag_marker = get_diag_marker(ws, pane, line_num);

                let label = match ln_mode {
                    LineNumberMode::Off => String::new(),
                    LineNumberMode::Absolute => format!("{:>4}", line_num + 1),
                    LineNumberMode::Relative => {
                        let dist = (line_num as isize - cursor.line as isize).unsigned_abs();
                        format!("{:>4}", dist)
                    }
                    LineNumberMode::Hybrid => {
                        if is_cursor_line {
                            format!("{:>4}", line_num + 1)
                        } else {
                            let dist = (line_num as isize - cursor.line as isize).unsigned_abs();
                            format!("{:>4}", dist)
                        }
                    }
                };

                if let Some(marker) = diag_marker {
                    let trim = &label[..label.len().saturating_sub(1)];
                    row_spans.push(RichSpan { text: trim.to_string(), color: num_color });
                    row_spans.push(marker);
                } else {
                    row_spans.push(RichSpan { text: label, color: num_color });
                }
                row_spans.push(RichSpan { text: " ".to_string(), color: DIM });
            }

            // Text content
            let raw_line = buf.get_line(line_num).unwrap_or_default();
            let expanded = expand_tabs(&raw_line, tab_width);

            if is_terminal {
                // Terminal: use styled cells
                if let Some(cells) = buf.get_styled_cells(line_num) {
                    let cell_spans = cells_to_rich_spans(cells);
                    row_spans.extend(cell_spans);
                    let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
                    let target = inner_cols + border_offset;
                    if used < target {
                        row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
                    }
                } else {
                    let display = truncate_str(&expanded, text_cols);
                    row_spans.push(RichSpan { text: display, color: FG });
                    let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
                    let target = inner_cols + border_offset;
                    if used < target {
                        row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
                    }
                }
            } else if let Some(sel) = selection {
                // Selection highlighting
                let (sel_start, sel_end) = sel.ordered();
                let spans = highlight_with_selection(&expanded, line_num, sel_start, sel_end, buf, syntax_theme);
                row_spans.extend(spans);
                let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
                let target = inner_cols + border_offset;
                if used < target {
                    row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
                }
            } else {
                // Syntax highlighting
                let hl_spans = buf.get_highlights(line_num);
                let text_spans = if let Some(spans) = hl_spans {
                    apply_syntax_highlights(&expanded, spans, syntax_theme)
                } else {
                    vec![RichSpan { text: truncate_str(&expanded, text_cols), color: FG }]
                };

                // Search highlighting overlay
                let text_spans = if let Some(pattern) = search_pattern {
                    if !pattern.is_empty() {
                        apply_search_highlight(&expanded, &text_spans, pattern)
                    } else {
                        text_spans
                    }
                } else {
                    text_spans
                };

                row_spans.extend(text_spans);
                let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
                let target = inner_cols + border_offset;
                if used < target {
                    row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
                }
            }

            // Cursor (overwrite the character at cursor position with inverse colors)
            if is_cursor_line && is_focused && !is_terminal {
                let cursor_display_col = display_col(&raw_line, cursor.column, tab_width);
                let cursor_abs_col = gutter_width + cursor_display_col + border_offset;
                apply_cursor_to_spans(&mut row_spans, cursor_abs_col);
            } else if is_cursor_line && is_focused && is_terminal {
                let cursor_abs_col = cursor.column + border_offset;
                apply_cursor_to_spans(&mut row_spans, cursor_abs_col);
            }

            line_num += 1;
        } else if !is_terminal {
            // Tilde rows
            if gutter_width > 0 {
                row_spans.push(RichSpan { text: "   ~ ".to_string(), color: TILDE_BLUE });
            } else {
                row_spans.push(RichSpan { text: "~".to_string(), color: TILDE_BLUE });
            }
            let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
            let target = inner_cols + border_offset;
            if used < target {
                row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
            }
        } else {
            let used: usize = row_spans.iter().map(|s| s.text.chars().count()).sum();
            let target = inner_cols + border_offset;
            if used < target {
                row_spans.push(RichSpan { text: " ".repeat(target - used), color: FG });
            }
        }

        if draw_border {
            row_spans.push(RichSpan { text: "│".to_string(), color: border_color });
        }
        lines.push(row_spans);
    }

    if draw_border {
        let bottom_row = {
            let mut spans = Vec::new();
            spans.push(RichSpan { text: "└".to_string(), color: border_color });
            if cols > 2 {
                spans.push(RichSpan { text: "─".repeat(cols - 2), color: border_color });
            }
            spans.push(RichSpan { text: "┘".to_string(), color: border_color });
            spans
        };
        lines.push(bottom_row);
    }

    // Pad if pane is shorter than allocated rows
    while lines.len() < rows {
        lines.push(vec![RichSpan { text: " ".repeat(cols), color: DIM }]);
    }

    lines
}

// ── Tab bar ───────────────────────────────────────────────────────────────────

pub(super) fn render_tab_bar(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
    let mut spans = Vec::new();
    for (i, ws) in editor.tabs.iter().enumerate() {
        let is_active = i == editor.active_tab;
        let accent = TAB_ACCENTS[i % TAB_ACCENTS.len()];
        let _color = if is_active { Color::rgb(30, 30, 30) } else { accent };
        // For active, we can't set bg per-glyph easily in glyphon, so use brackets
        if is_active {
            spans.push(RichSpan { text: "▌".to_string(), color: accent });
            spans.push(RichSpan { text: format!(" {} ", ws.name), color: accent });
            spans.push(RichSpan { text: "▐".to_string(), color: accent });
        } else {
            spans.push(RichSpan { text: format!(" {} ", ws.name), color: accent });
        }
        if i + 1 < editor.tabs.len() {
            spans.push(RichSpan { text: "│".to_string(), color: DIM });
        }
    }
    // Pad to fill width
    let used: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if used < cols {
        spans.push(RichSpan { text: " ".repeat(cols - used), color: DIM });
    }
    spans
}

// ── Status bar ────────────────────────────────────────────────────────────────

pub(super) fn render_status_bar(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
    let info = editor.status_bar_info();
    let sb_config = &editor.config.status_bar;

    let left = info.format_left(&sb_config.left);
    let right = info.format_right(&sb_config.right);

    let padding = cols.saturating_sub(left.len() + right.len());
    let full = format!("{}{:padding$}{}", left, "", right, padding = padding);

    let mode_name = info.mode_name;

    // Colorize mode name
    let mode_color = match editor.mode {
        EditorMode::Normal => BLUE,
        EditorMode::Insert => GREEN,
        EditorMode::Visual | EditorMode::VisualBlock => YELLOW,
        EditorMode::Command => YELLOW,
        EditorMode::Replace => RED,
    };

    let mut spans = Vec::new();
    spans.push(RichSpan { text: format!(" {} ", mode_name), color: mode_color });
    let rest = &full[mode_name.len() + 2..]; // skip " MODE "
    spans.push(RichSpan { text: rest.to_string(), color: FG });

    // Pad
    let used: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if used < cols {
        spans.push(RichSpan { text: " ".repeat(cols - used), color: FG });
    }

    spans
}

// ── Command / search line ─────────────────────────────────────────────────────

pub(super) fn render_command_line(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
    let _text = format!(":{}", editor.command_buffer);
    let mut spans = vec![
        RichSpan { text: ":".to_string(), color: YELLOW },
        RichSpan { text: editor.command_buffer.clone(), color: FG },
    ];
    let used: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if used < cols {
        spans.push(RichSpan { text: " ".repeat(cols - used), color: FG });
    }
    spans
}

pub(super) fn render_search_line(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
    let pattern = editor.search.pattern.as_deref().unwrap_or("");
    let mut spans = vec![
        RichSpan { text: "/".to_string(), color: YELLOW },
        RichSpan { text: pattern.to_string(), color: FG },
    ];
    let used: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if used < cols {
        spans.push(RichSpan { text: " ".repeat(cols - used), color: FG });
    }
    spans
}

// ── Explorer ──────────────────────────────────────────────────────────────────

pub(super) fn render_explorer(editor: &EditorState, cols: usize, rows: usize) -> Vec<Vec<RichSpan>> {
    let ws = &editor.tabs[editor.active_tab];
    let Some(explorer) = &ws.explorer else {
        return (0..rows).map(|_| vec![RichSpan { text: " ".repeat(cols), color: DIM }]).collect();
    };

    let scroll = if explorer.cursor >= rows { explorer.cursor - rows + 1 } else { 0 };
    let mut lines = Vec::with_capacity(rows);

    for i in 0..rows {
        let idx = scroll + i;
        if let Some((name, is_dir, expanded, depth)) = explorer.entry_display(idx) {
            let is_cursor = idx == explorer.cursor;
            let indent = "  ".repeat(depth);
            let icon = if is_dir {
                if expanded { "▼ " } else { "▶ " }
            } else {
                "  "
            };
            let display = format!("{}{}{}", indent, icon, name);
            let color = if is_cursor && ws.explorer_focused {
                FG // brighter when selected
            } else if is_dir {
                EXPLORER_DIR
            } else {
                DIM
            };
            let text = truncate_str(&display, cols);
            let text_len = text.chars().count();
            let mut row = vec![RichSpan { text, color }];
            if text_len < cols {
                row.push(RichSpan { text: " ".repeat(cols - text_len), color: DIM });
            }
            lines.push(row);
        } else {
            lines.push(vec![RichSpan { text: " ".repeat(cols), color: DIM }]);
        }
    }

    lines
}

// ── Viewport adjustment ──────────────────────────────────────────────────────

/// Adjust viewport_offset for all panes so the cursor stays visible.
/// This mirrors the TUI renderer's scroll logic but runs as a separate pass
/// since the GUI render functions take immutable references.
pub(super) fn adjust_viewports(editor: &mut EditorState, cols: usize, rows: usize) {
    let has_tabs = editor.tabs.len() > 1;
    let in_command = editor.mode == EditorMode::Command;
    let in_search = editor.search.active;
    let tab_bar_rows = if has_tabs { 1 } else { 0 };
    let bottom_rows = 1 + if in_command || in_search { 1 } else { 0 };
    let main_rows = rows.saturating_sub(tab_bar_rows + bottom_rows);

    let idx = editor.active_tab;
    let ws = &editor.tabs[idx];
    let explorer_cols = if ws.explorer.is_some() { 30usize.min(cols / 3) } else { 0 };
    let pane_cols = cols.saturating_sub(explorer_cols);

    let area = novim_types::Rect::new(0, 0, pane_cols as u16, main_rows as u16);
    let layouts = editor.tabs[idx].panes.layout(area);

    let multi_pane = layouts.len() > 1;
    for (pane_id, rect) in &layouts {
        let draw_border = multi_pane;
        let border_overhead = if draw_border { 2 } else { 0 };
        let available_height = (rect.height as usize).saturating_sub(border_overhead);

        if let Some(pane) = editor.tabs[idx].panes.get_pane_mut(*pane_id) {
            let buf = pane.content.as_buffer_like();
            if buf.is_terminal() {
                continue;
            }
            let cursor = buf.cursor();

            if cursor.line < pane.viewport_offset {
                pane.viewport_offset = cursor.line;
            } else if available_height > 0 && cursor.line >= pane.viewport_offset + available_height {
                pane.viewport_offset = cursor.line.saturating_sub(available_height - 1);
            }
        }
    }
}

// ── Welcome screen ───────────────────────────────────────────────────────────

/// Render the welcome/splash screen centered in the GPU window.
pub(super) fn render_welcome_screen(state: &mut crate::WindowState, cols: usize, rows: usize) {
    let wlines = welcome::welcome_lines();
    let content_height = wlines.len();
    let start_y = rows.saturating_sub(content_height) / 2;
    let max_visual = wlines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);

    let logo_color = Color::rgb(95, 175, 255);   // soft blue
    let version_color = Color::rgb(120, 120, 120);
    let key_color = Color::rgb(95, 175, 255);
    let desc_color = Color::rgb(208, 208, 208);

    let mut screen_lines: Vec<Vec<RichSpan>> = Vec::with_capacity(rows);

    for _ in 0..start_y {
        screen_lines.push(vec![RichSpan { text: " ".repeat(cols), color: FG }]);
    }

    for wl in &wlines {
        let pad = cols / 2 - max_visual.min(cols) / 2;
        let padding = " ".repeat(pad);

        let line = match wl.kind {
            "logo" => vec![
                RichSpan { text: padding, color: FG },
                RichSpan { text: wl.text.clone(), color: logo_color },
            ],
            "version" => vec![
                RichSpan { text: padding, color: FG },
                RichSpan { text: wl.text.clone(), color: version_color },
            ],
            "shortcut" => {
                if let Some(pos) = wl.text.find("   ") {
                    let key_part = &wl.text[..pos];
                    let desc_part = &wl.text[pos + 3..];
                    vec![
                        RichSpan { text: padding, color: FG },
                        RichSpan { text: key_part.to_string(), color: key_color },
                        RichSpan { text: format!("   {}", desc_part), color: desc_color },
                    ]
                } else {
                    vec![
                        RichSpan { text: padding, color: FG },
                        RichSpan { text: wl.text.clone(), color: desc_color },
                    ]
                }
            }
            _ => vec![RichSpan { text: padding, color: FG }],
        };
        screen_lines.push(line);
    }

    // Pad to fill screen
    while screen_lines.len() < rows {
        screen_lines.push(vec![RichSpan { text: " ".repeat(cols), color: FG }]);
    }

    // Convert to flat rich_spans
    let mut rich_spans: Vec<(String, Color)> = Vec::with_capacity(rows * 4);
    for (i, line) in screen_lines.drain(..).enumerate() {
        if i > 0 {
            rich_spans.push(("\n".to_string(), FG));
        }
        for span in line {
            if !span.text.is_empty() {
                rich_spans.push((span.text, span.color));
            }
        }
    }

    super::submit_frame(state, &rich_spans, &[]);
}
