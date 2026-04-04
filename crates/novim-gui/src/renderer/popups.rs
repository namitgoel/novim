//! Popup overlay rendering: finder, help, buffer list, workspace list, plugin popup.

use glyphon::Color;
use novim_core::editor::EditorState;
use novim_core::help::{help_entries, help_entry_to_plain};

use super::theme::*;

// ── Popup overlay constants ──────────────────────────────────────────────────

const POPUP_BORDER_COLOR: Color = Color::rgb(80, 180, 230);
const POPUP_TEXT: Color = Color::rgb(220, 220, 220);
const POPUP_DIM_COLOR: Color = Color::rgb(140, 140, 140);
const POPUP_HIGHLIGHT: Color = Color::rgb(255, 255, 255);
const POPUP_BG_F32: [f32; 4] = [0.17, 0.17, 0.22, 1.0]; // ~(45,45,55)
const POPUP_SELECTED_F32: [f32; 4] = [0.10, 0.30, 0.50, 1.0]; // blue highlight

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Apply popup overlays (finder, help, buffer list, workspace list) on top of screen content.
pub(super) fn apply_popup_overlays(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    state: &crate::WindowState,
    cols: usize,
    rows: usize,
) {
    let cell_w = state.gpu.cell_width;
    let cell_h = state.gpu.cell_height;
    let editor = &state.editor;

    if editor.finder.visible {
        overlay_finder(screen_lines, bg_rects, editor, cols, rows, cell_w, cell_h);
    } else if editor.show_help {
        overlay_help(screen_lines, bg_rects, editor, cols, rows, cell_w, cell_h);
    } else if editor.tabs[editor.active_tab].show_buffer_list {
        overlay_buffer_list(screen_lines, bg_rects, editor, cols, rows, cell_w, cell_h);
    } else if editor.show_workspace_list {
        overlay_workspace_list(screen_lines, bg_rects, editor, cols, rows, cell_w, cell_h);
    } else if editor.plugin_popup.is_some() {
        overlay_plugin_popup(screen_lines, bg_rects, editor, cols, rows, cell_w, cell_h);
    }
}

/// Stamp a single text line into screen_lines at (row, col), with bg rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn stamp_line(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    row: usize,
    col: usize,
    width: usize,
    text: &str,
    fg: Color,
    bg: [f32; 4],
    cell_w: f32,
    cell_h: f32,
) {
    if row >= screen_lines.len() { return; }
    // Background rect for the whole line
    bg_rects.push(crate::gpu::BgRect {
        x: col as f32 * cell_w,
        y: row as f32 * cell_h,
        w: width as f32 * cell_w,
        h: cell_h,
        color: bg,
    });
    // Truncate or pad text to width
    let display: String = {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() >= width {
            chars[..width].iter().collect()
        } else {
            let mut s: String = chars.into_iter().collect();
            while s.chars().count() < width { s.push(' '); }
            s
        }
    };
    // Replace the line content at this position
    screen_lines[row] = build_line_with_overlay(
        &screen_lines[row], col, width, &display, fg,
    );
}

/// Build a new line by overlaying text at a given column range,
/// preserving original span colors for content outside the overlay.
pub(super) fn build_line_with_overlay(
    existing: &[RichSpan],
    col: usize,
    width: usize,
    text: &str,
    fg: Color,
) -> Vec<RichSpan> {
    let mut result = Vec::new();
    let end_col = col + width;
    let mut pos = 0;

    // Content before the overlay — preserve original colors
    for span in existing {
        let mut chunk = String::new();
        for ch in span.text.chars() {
            if ch == '\n' { continue; }
            if pos < col {
                chunk.push(ch);
            }
            pos += 1;
        }
        if !chunk.is_empty() {
            result.push(RichSpan { text: chunk, color: span.color });
        }
        if pos >= col { break; }
    }
    // Pad if existing content is shorter than col
    let before_len: usize = result.iter().map(|s| s.text.chars().count()).sum();
    if before_len < col {
        result.push(RichSpan { text: " ".repeat(col - before_len), color: FG });
    }

    // The overlay itself
    result.push(RichSpan { text: text.to_string(), color: fg });

    // Content after the overlay — preserve original colors
    pos = 0;
    for span in existing {
        let mut chunk = String::new();
        for ch in span.text.chars() {
            if ch == '\n' { continue; }
            if pos >= end_col {
                chunk.push(ch);
            }
            pos += 1;
        }
        if !chunk.is_empty() {
            result.push(RichSpan { text: chunk, color: span.color });
        }
    }
    result
}

// ── Popup overlays ───────────────────────────────────────────────────────────

/// File finder popup overlay.
pub(super) fn overlay_finder(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    editor: &EditorState,
    cols: usize,
    rows: usize,
    cell_w: f32,
    cell_h: f32,
) {
    let popup_w = (cols * 3 / 5).max(40).min(cols.saturating_sub(4));
    let popup_h = (rows * 3 / 5).max(10).min(rows.saturating_sub(4));
    let x = (cols.saturating_sub(popup_w)) / 2;
    let y = (rows.saturating_sub(popup_h)) / 2;

    // Top border
    let title = format!(" Find Files ({}) ", editor.finder.results.len());
    let border_top = format!("╭{}{}╮",
        title,
        "─".repeat(popup_w.saturating_sub(title.len() + 2)),
    );
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    // Search input line
    let input = format!("│ > {}{}│",
        editor.finder.query,
        " ".repeat(popup_w.saturating_sub(editor.finder.query.len() + 6).max(0)),
    );
    stamp_line(screen_lines, bg_rects, y + 1, x, popup_w, &input, POPUP_HIGHLIGHT, POPUP_BG_F32, cell_w, cell_h);

    // Separator
    let sep = format!("├{}┤", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + 2, x, popup_w, &sep, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    // Results
    let visible_count = popup_h.saturating_sub(4); // top border + input + separator + bottom border
    let scroll = if editor.finder.selected >= visible_count {
        editor.finder.selected - visible_count + 1
    } else {
        0
    };

    for i in 0..visible_count {
        let row = y + 3 + i;
        let idx = scroll + i;
        if idx < editor.finder.results.len() {
            let is_selected = idx == editor.finder.selected;
            let result = &editor.finder.results[idx];
            let max_len = popup_w.saturating_sub(4);
            let display = if result.display.len() > max_len {
                format!("...{}", &result.display[result.display.len().saturating_sub(max_len.saturating_sub(3))..])
            } else {
                result.display.clone()
            };
            let line = format!("│ {}{}│",
                display,
                " ".repeat(popup_w.saturating_sub(display.len() + 4).max(0)),
            );
            let bg = if is_selected { POPUP_SELECTED_F32 } else { POPUP_BG_F32 };
            let fg = if is_selected { POPUP_HIGHLIGHT } else { POPUP_TEXT };
            stamp_line(screen_lines, bg_rects, row, x, popup_w, &line, fg, bg, cell_w, cell_h);
        } else {
            let empty = format!("│{}│", " ".repeat(popup_w.saturating_sub(2)));
            stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM_COLOR, POPUP_BG_F32, cell_w, cell_h);
        }
    }

    // Bottom border
    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);
}

/// Help popup overlay.
pub(super) fn overlay_help(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    editor: &EditorState,
    cols: usize,
    rows: usize,
    cell_w: f32,
    cell_h: f32,
) {
    let popup_w = 60usize.min(cols.saturating_sub(4));
    let popup_h = (rows * 3 / 4).min(rows.saturating_sub(4));
    let x = (cols.saturating_sub(popup_w)) / 2;
    let y = (rows.saturating_sub(popup_h)) / 2;

    let entries = help_entries();
    let help_lines: Vec<String> = entries.iter().map(|e| help_entry_to_plain(e).1).collect();

    let title = " Help ";
    let border_top = format!("╭{}{}╮", title, "─".repeat(popup_w.saturating_sub(title.len() + 2)));
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    let visible = popup_h.saturating_sub(2);
    let scroll = editor.help_scroll;
    for i in 0..visible {
        let row = y + 1 + i;
        let idx = scroll + i;
        let text = if idx < help_lines.len() { &help_lines[idx] } else { "" };
        let line = format!("│ {}{}│",
            text,
            " ".repeat(popup_w.saturating_sub(text.len() + 4).max(0)),
        );
        let fg = if text.starts_with("──") { POPUP_BORDER_COLOR } else { POPUP_TEXT };
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &line, fg, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);
}

/// Buffer list popup overlay.
pub(super) fn overlay_buffer_list(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    editor: &EditorState,
    cols: usize,
    rows: usize,
    cell_w: f32,
    cell_h: f32,
) {
    let ws = &editor.tabs[editor.active_tab];
    let core_area = novim_types::Rect::new(0, 0, cols as u16, rows as u16);
    let layouts = ws.panes.layout(core_area);
    let buffers: Vec<String> = layouts.iter().map(|(id, _)| {
        ws.panes.get_pane(*id)
            .map(|p| p.content.as_buffer_like().display_name().to_string())
            .unwrap_or_else(|| format!("pane {}", id))
    }).collect();
    let popup_w = 50usize.min(cols.saturating_sub(4));
    let popup_h = (buffers.len() + 4).min(rows.saturating_sub(4));
    let x = (cols.saturating_sub(popup_w)) / 2;
    let y = (rows.saturating_sub(popup_h)) / 2;

    let title = " Buffers ";
    let border_top = format!("╭{}{}╮", title, "─".repeat(popup_w.saturating_sub(title.len() + 2)));
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    for (i, name) in buffers.iter().enumerate() {
        let row = y + 1 + i;
        if row >= y + popup_h - 1 { break; }
        let line = format!("│ {}{}│",
            name,
            " ".repeat(popup_w.saturating_sub(name.len() + 4).max(0)),
        );
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &line, POPUP_TEXT, POPUP_BG_F32, cell_w, cell_h);
    }

    // Fill remaining rows
    for row in (y + 1 + buffers.len())..(y + popup_h - 1) {
        let empty = format!("│{}│", " ".repeat(popup_w.saturating_sub(2)));
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM_COLOR, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);
}

/// Workspace list popup overlay.
pub(super) fn overlay_workspace_list(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    editor: &EditorState,
    cols: usize,
    rows: usize,
    cell_w: f32,
    cell_h: f32,
) {
    let popup_w = 50usize.min(cols.saturating_sub(4));
    let popup_h = (editor.tabs.len() + 4).min(rows.saturating_sub(4));
    let x = (cols.saturating_sub(popup_w)) / 2;
    let y = (rows.saturating_sub(popup_h)) / 2;

    let title = " Workspaces ";
    let border_top = format!("╭{}{}╮", title, "─".repeat(popup_w.saturating_sub(title.len() + 2)));
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    for (i, ws) in editor.tabs.iter().enumerate() {
        let row = y + 1 + i;
        if row >= y + popup_h - 1 { break; }
        let is_active = i == editor.active_tab;
        let marker = if is_active { "* " } else { "  " };
        let text = format!("{}{}", marker, ws.name);
        let line = format!("│ {}{}│",
            text,
            " ".repeat(popup_w.saturating_sub(text.len() + 4).max(0)),
        );
        let bg = if i == editor.workspace_list_selected { POPUP_SELECTED_F32 } else { POPUP_BG_F32 };
        let fg = if is_active { POPUP_HIGHLIGHT } else { POPUP_TEXT };
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &line, fg, bg, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);
}

/// Plugin popup overlay.
pub(super) fn overlay_plugin_popup(
    screen_lines: &mut [Vec<RichSpan>],
    bg_rects: &mut Vec<crate::gpu::BgRect>,
    editor: &EditorState,
    cols: usize,
    rows: usize,
    cell_w: f32,
    cell_h: f32,
) {
    let popup = match &editor.plugin_popup {
        Some(p) => p,
        None => return,
    };
    let selectable = popup.on_select.is_some();
    let auto_w = popup.lines.iter().map(|l| l.len()).max().unwrap_or(20).max(popup.title.len() + 4) + 4;
    let auto_h = popup.lines.len() + 2;
    let popup_w = popup.width.map(|w| w as usize).unwrap_or(auto_w).clamp(10, cols.saturating_sub(4));
    let popup_h = popup.height.map(|h| h as usize).unwrap_or(auto_h).clamp(4, rows.saturating_sub(4));
    let x = (cols.saturating_sub(popup_w)) / 2;
    let y = (rows.saturating_sub(popup_h)) / 2;

    let title = format!(" {} ", popup.title);
    let border_top = format!("╭{}{}╮", title, "─".repeat(popup_w.saturating_sub(title.len() + 2)));
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);

    let visible_h = popup_h.saturating_sub(2);
    let max_scroll = popup.lines.len().saturating_sub(visible_h);
    let scroll = popup.scroll.min(max_scroll);

    for (vi, i) in (scroll..popup.lines.len()).take(visible_h).enumerate() {
        let row = y + 1 + vi;
        let line_text = &popup.lines[i];
        let prefix = if selectable && i == popup.selected { "> " } else { "  " };
        let text = format!("{}{}", prefix, line_text);
        let padded = format!("│ {}{}│", text, " ".repeat(popup_w.saturating_sub(text.len() + 4).max(0)));
        let bg = if selectable && i == popup.selected { POPUP_SELECTED_F32 } else { POPUP_BG_F32 };
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &padded, POPUP_TEXT, bg, cell_w, cell_h);
    }

    // Fill remaining empty rows
    for vi in popup.lines.len().saturating_sub(scroll)..visible_h {
        let row = y + 1 + vi;
        let empty = format!("│{}│", " ".repeat(popup_w.saturating_sub(2)));
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM_COLOR, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER_COLOR, POPUP_BG_F32, cell_w, cell_h);
}
