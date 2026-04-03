//! GPU renderer — converts EditorState into glyphon text areas.
//!
//! This mirrors the TUI renderer logic but outputs `(text, Attrs)` spans
//! that glyphon renders with per-character syntax colors.

use glyphon::{Attrs, Color, Family, Metrics, Shaping, TextArea, TextBounds};
use novim_core::buffer::BufferLike;
use novim_core::config::{self, SyntaxTheme};
use novim_core::editor::{EditorState, LineNumberMode};
use novim_core::highlight::HighlightGroup;
use novim_core::pane::{Pane, PaneContent};
use novim_core::welcome;
use novim_types::EditorMode;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// ── Colors (loaded from theme config) ─────────────────────────────────────────

/// Default foreground — used as fallback throughout the renderer.
const FG: Color = Color::rgb(220, 220, 220);
const DIM: Color = Color::rgb(120, 120, 120);

/// Load a glyphon Color from a theme config string.
#[allow(dead_code)]
fn theme_color(s: &str) -> Color {
    config_color_to_glyphon(config::parse_color(s))
}

/// Load a [f32;4] RGBA from a theme config string (for bg_rects).
#[allow(dead_code)]
fn theme_color_f32(s: &str) -> [f32; 4] {
    let Color(rgba) = theme_color(s);
    let bytes = rgba.to_be_bytes();
    [bytes[0] as f32 / 255.0, bytes[1] as f32 / 255.0, bytes[2] as f32 / 255.0, 1.0]
}

/// Cached theme colors for the GUI renderer, derived from config.
#[allow(dead_code)]
struct GuiTheme {
    fg: Color,
    cursor_bg: Color,
    cursor_fg: Color,
    selection_bg: Color,
    search_match: Color,
    line_num: Color,
    line_num_active: Color,
    tilde: Color,
    focused_border: Color,
    unfocused_border: Color,
    status_bar_bg: Color,
    mode_normal: Color,
    mode_insert: Color,
    mode_visual: Color,
    mode_command: Color,
    explorer_dir: Color,
    diag_error: Color,
    diag_warning: Color,
    tab_active_bg: Color,
    tab_inactive_bg: Color,
    popup_bg: Color,
    popup_border: Color,
    popup_selected_bg: Color,
    popup_bg_f32: [f32; 4],
    popup_selected_f32: [f32; 4],
    git_added: Color,
    git_modified: Color,
    git_deleted: Color,
}

impl GuiTheme {
    #[allow(dead_code)]
    fn from_config(theme: &config::ThemeConfig) -> Self {
        Self {
            fg: theme_color(&theme.foreground),
            cursor_bg: theme_color(&theme.cursor_bg),
            cursor_fg: theme_color(&theme.cursor_fg),
            selection_bg: theme_color(&theme.selection_bg),
            search_match: theme_color(&theme.search_match),
            line_num: theme_color(&theme.line_number),
            line_num_active: theme_color(&theme.current_line_number),
            tilde: theme_color(&theme.tilde),
            focused_border: theme_color(&theme.focused_border),
            unfocused_border: theme_color(&theme.unfocused_border),
            status_bar_bg: theme_color(&theme.status_bar_bg),
            mode_normal: theme_color(&theme.mode_normal),
            mode_insert: theme_color(&theme.mode_insert),
            mode_visual: theme_color(&theme.mode_visual),
            mode_command: theme_color(&theme.mode_command),
            explorer_dir: theme_color(&theme.explorer_dir),
            diag_error: theme_color(&theme.diag_error),
            diag_warning: theme_color(&theme.diag_warning),
            tab_active_bg: theme_color(&theme.tab_active_bg),
            tab_inactive_bg: theme_color(&theme.tab_inactive_bg),
            popup_bg: theme_color(&theme.popup_bg),
            popup_border: theme_color(&theme.popup_border),
            popup_selected_bg: theme_color(&theme.popup_selected_bg),
            popup_bg_f32: theme_color_f32(&theme.popup_bg),
            popup_selected_f32: theme_color_f32(&theme.popup_selected_bg),
            git_added: theme_color(&theme.git_added),
            git_modified: theme_color(&theme.git_modified),
            git_deleted: theme_color(&theme.git_deleted),
        }
    }
}

// Legacy constants — still referenced by rendering code not yet migrated to GuiTheme.
#[allow(dead_code)]
const YELLOW: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
const RED: Color = Color::rgb(224, 108, 117);
#[allow(dead_code)]
const BLUE: Color = Color::rgb(97, 175, 239);
#[allow(dead_code)]
const GREEN: Color = Color::rgb(152, 195, 121);
#[allow(dead_code)]
const TILDE_BLUE: Color = Color::rgb(65, 105, 225);
#[allow(dead_code)]
const CURSOR_BG: Color = Color::rgb(200, 200, 200);
#[allow(dead_code)]
const CURSOR_FG: Color = Color::rgb(30, 30, 30);
#[allow(dead_code)]
const SELECTION_BG: Color = Color::rgb(60, 80, 120);
#[allow(dead_code)]
const LINE_NUM: Color = Color::rgb(100, 100, 100);
#[allow(dead_code)]
const LINE_NUM_ACTIVE: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
const SEARCH_MATCH_BG: Color = Color::rgb(100, 80, 40);
#[allow(dead_code)]
const EXPLORER_DIR: Color = Color::rgb(97, 175, 239);
#[allow(dead_code)]
const EXPLORER_CURSOR_BG: Color = Color::rgb(60, 60, 70);
#[allow(dead_code)]
const DIAG_ERROR_FG: Color = Color::rgb(224, 108, 117);
#[allow(dead_code)]
const DIAG_WARN_FG: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
const BG_STATUS: Color = Color::rgb(60, 60, 66);
#[allow(dead_code)]
const BG_TAB_ACTIVE: Color = Color::rgb(80, 80, 100);
#[allow(dead_code)]
const BG_TAB_INACTIVE: Color = Color::rgb(40, 40, 46);
#[allow(dead_code)]
const POPUP_BG: Color = Color::rgb(45, 45, 55);
#[allow(dead_code)]
const POPUP_SELECTED_BG: Color = Color::rgb(70, 70, 90);
#[allow(dead_code)]
const COMPLETION_BG: Color = Color::rgb(40, 40, 50);

/// Tab bar accent palette.
const TAB_ACCENTS: &[Color] = &[
    Color::rgb(97, 175, 239),  // blue
    Color::rgb(152, 195, 121), // green
    Color::rgb(198, 120, 221), // purple
    Color::rgb(224, 108, 117), // red
    Color::rgb(229, 192, 123), // gold
    Color::rgb(86, 182, 194),  // teal
];

// ── Span builder utility ──────────────────────────────────────────────────────

/// A colored text span for rich text rendering.
struct RichSpan {
    text: String,
    color: Color,
}

fn mono_attrs(color: Color) -> Attrs<'static> {
    Attrs::new().family(Family::Monospace).color(color)
}

// ── Main render entry ─────────────────────────────────────────────────────────

/// Compose the full editor UI into one or more glyphon TextBuffers and render.
pub fn render(state: &mut crate::WindowState) {
    let cols = state.gpu.grid_cols() as usize;
    let rows = state.gpu.grid_rows() as usize;
    if cols == 0 || rows == 0 {
        return;
    }

    // Welcome screen (TUI-only, but keep for safety).
    if state.editor.show_welcome {
        render_welcome_screen(state, cols, rows);
        return;
    }

    state.editor.focused_buf_mut().reparse_highlights();

    // Dispatch: terminal mode (no chrome) vs editor mode (full chrome)
    let editor = &state.editor;
    let ws = &editor.tabs[editor.active_tab];
    let is_pure_terminal = editor.tabs.len() == 1
        && editor.mode != EditorMode::Command
        && !editor.search.active
        && ws.explorer.is_none()
        && ws.panes.pane_count() == 1
        && ws.panes.focused_pane().content.as_buffer_like().is_terminal();

    if is_pure_terminal {
        render_terminal_mode(state, cols, rows);
    } else {
        render_editor_mode(state, cols, rows);
    }
}

// ── Render paths ─────────────────────────────────────────────────────────────

/// Terminal mode: single terminal pane, full screen, no chrome.
fn render_terminal_mode(state: &mut crate::WindowState, cols: usize, rows: usize) {
    adjust_viewports(&mut state.editor, cols, rows);

    let pane_lines = render_pane_area(&state.editor, cols, rows);
    let mut screen_lines: Vec<Vec<RichSpan>> = Vec::with_capacity(rows);
    for row in 0..rows {
        if row < pane_lines.len() {
            screen_lines.push(pane_lines[row].clone());
        } else {
            screen_lines.push(vec![RichSpan { text: " ".repeat(cols), color: FG }]);
        }
    }

    let mut bg_rects: Vec<crate::gpu::BgRect> = Vec::new();
    apply_popup_overlays(&mut screen_lines, &mut bg_rects, state, cols, rows);
    let rich_spans = flatten_screen_lines(screen_lines);
    submit_frame(state, &rich_spans, &bg_rects);
}

/// Editor mode: tab bar, status bar, explorer, pane borders.
fn render_editor_mode(state: &mut crate::WindowState, cols: usize, rows: usize) {
    adjust_viewports(&mut state.editor, cols, rows);

    let editor = &state.editor;
    let has_tabs = editor.tabs.len() > 1;
    let in_command = editor.mode == EditorMode::Command;
    let in_search = editor.search.active;

    // Layout: [tab_bar?] [main_area] [status_bar] [cmd_line?]
    let tab_bar_rows = if has_tabs { 1usize } else { 0 };
    let bottom_rows = 1 + if in_command || in_search { 1 } else { 0 };
    let main_rows = rows.saturating_sub(tab_bar_rows + bottom_rows);

    let mut screen_lines: Vec<Vec<RichSpan>> = Vec::with_capacity(rows);

    // Tab bar
    if has_tabs {
        screen_lines.push(render_tab_bar(editor, cols));
    }

    // Main area: explorer + panes
    let ws = &editor.tabs[editor.active_tab];
    let explorer_cols = if ws.explorer.is_some() { 30usize.min(cols / 3) } else { 0 };
    let pane_cols = cols.saturating_sub(explorer_cols);

    let pane_lines = render_pane_area(editor, pane_cols, main_rows);
    let explorer_lines = if explorer_cols > 0 {
        render_explorer(editor, explorer_cols, main_rows)
    } else {
        Vec::new()
    };

    // Merge explorer + pane lines side by side
    for row in 0..main_rows {
        let mut line = Vec::new();
        if explorer_cols > 0 {
            if row < explorer_lines.len() {
                line.extend(explorer_lines[row].iter().cloned());
            } else {
                line.push(RichSpan { text: " ".repeat(explorer_cols), color: DIM });
            }
            line.push(RichSpan { text: "│".to_string(), color: DIM });
        }
        if row < pane_lines.len() {
            line.extend(pane_lines[row].iter().cloned());
        }
        screen_lines.push(line);
    }

    // Status bar
    screen_lines.push(render_status_bar(editor, cols));

    // Command / search line
    if in_command {
        screen_lines.push(render_command_line(editor, cols));
    } else if in_search {
        screen_lines.push(render_search_line(editor, cols));
    }

    // Pad to fill screen
    while screen_lines.len() < rows {
        screen_lines.push(vec![RichSpan { text: " ".repeat(cols), color: FG }]);
    }

    // Popup overlays
    let mut bg_rects: Vec<crate::gpu::BgRect> = Vec::new();
    apply_popup_overlays(&mut screen_lines, &mut bg_rects, state, cols, rows);
    let rich_spans = flatten_screen_lines(screen_lines);
    submit_frame(state, &rich_spans, &bg_rects);
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Apply popup overlays (finder, help, buffer list, workspace list) on top of screen content.
fn apply_popup_overlays(
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

/// Convert screen lines to flat (text, color) spans for glyphon.
fn flatten_screen_lines(mut screen_lines: Vec<Vec<RichSpan>>) -> Vec<(String, Color)> {
    let mut rich_spans: Vec<(String, Color)> = Vec::with_capacity(screen_lines.len() * 8);
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
    rich_spans
}

// ── Popup overlay helpers ─────────────────────────────────────────────────────

const POPUP_BORDER: Color = Color::rgb(80, 180, 230);
const POPUP_TEXT: Color = Color::rgb(220, 220, 220);
const POPUP_DIM: Color = Color::rgb(140, 140, 140);
const POPUP_HIGHLIGHT: Color = Color::rgb(255, 255, 255);
const POPUP_BG_F32: [f32; 4] = [0.17, 0.17, 0.22, 1.0]; // ~(45,45,55)
const POPUP_SELECTED_F32: [f32; 4] = [0.10, 0.30, 0.50, 1.0]; // blue highlight

/// Stamp a single text line into screen_lines at (row, col), with bg rect.
#[allow(clippy::too_many_arguments)]
fn stamp_line(
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
fn build_line_with_overlay(
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

/// File finder popup overlay.
fn overlay_finder(
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
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

    // Search input line
    let input = format!("│ > {}{}│",
        editor.finder.query,
        " ".repeat(popup_w.saturating_sub(editor.finder.query.len() + 6).max(0)),
    );
    stamp_line(screen_lines, bg_rects, y + 1, x, popup_w, &input, POPUP_HIGHLIGHT, POPUP_BG_F32, cell_w, cell_h);

    // Separator
    let sep = format!("├{}┤", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + 2, x, popup_w, &sep, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

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
            stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM, POPUP_BG_F32, cell_w, cell_h);
        }
    }

    // Bottom border
    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);
}

/// Help popup overlay.
fn overlay_help(
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

    let help_lines = vec![
        "Novim Shortcuts",
        "",
        "── Navigation ──",
        "h/j/k/l        Move cursor",
        "Ctrl+U/D       Scroll half page",
        "gg / G         Top / Bottom",
        "",
        "── Editing ──",
        "i / a          Insert / Append",
        "o / O          Open line below / above",
        "dd             Delete line",
        "yy / p         Yank / Paste",
        "u / Ctrl+R     Undo / Redo",
        "",
        "── Cmd Shortcuts (GUI) ──",
        "Cmd+P / Cmd+F  File finder",
        "Cmd+E          Toggle explorer",
        "Cmd+T          New terminal",
        "Cmd+S          Save",
        "Cmd+W ...      Pane commands",
        "Cmd+N          Next tab",
        "Cmd+Q          Quit",
        "Cmd+/          Search",
        "Cmd+?          This help",
        "",
        "── Pane Commands (Ctrl+W) ──",
        "Ctrl+W s/v     Split H / V",
        "Ctrl+W h/j/k/l Focus direction",
        "Ctrl+W q       Close pane",
        "Ctrl+W t       Open terminal",
        "Ctrl+W f       File finder",
        "",
        "── Command mode ──",
        ":w / :q / :wq  Save / Quit",
        ":e <file>      Open file",
        ":term          Open terminal",
        "",
        "Press Esc or ? to close",
    ];

    let title = " Help ";
    let border_top = format!("╭{}{}╮", title, "─".repeat(popup_w.saturating_sub(title.len() + 2)));
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

    let visible = popup_h.saturating_sub(2);
    let scroll = editor.help_scroll;
    for i in 0..visible {
        let row = y + 1 + i;
        let idx = scroll + i;
        let text = if idx < help_lines.len() { help_lines[idx] } else { "" };
        let line = format!("│ {}{}│",
            text,
            " ".repeat(popup_w.saturating_sub(text.len() + 4).max(0)),
        );
        let fg = if text.starts_with("──") { POPUP_BORDER } else { POPUP_TEXT };
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &line, fg, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);
}

/// Buffer list popup overlay.
fn overlay_buffer_list(
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
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

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
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);
}

/// Workspace list popup overlay.
fn overlay_workspace_list(
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
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

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
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);
}

/// Plugin popup overlay.
fn overlay_plugin_popup(
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
    stamp_line(screen_lines, bg_rects, y, x, popup_w, &border_top, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);

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
        stamp_line(screen_lines, bg_rects, row, x, popup_w, &empty, POPUP_DIM, POPUP_BG_F32, cell_w, cell_h);
    }

    let border_bottom = format!("╰{}╯", "─".repeat(popup_w.saturating_sub(2)));
    stamp_line(screen_lines, bg_rects, y + popup_h - 1, x, popup_w, &border_bottom, POPUP_BORDER, POPUP_BG_F32, cell_w, cell_h);
}

/// Content-hash cache + glyphon submission shared by both render paths.
fn submit_frame(state: &mut crate::WindowState, rich_spans: &[(String, Color)], bg_rects: &[crate::gpu::BgRect]) {
    let frame_hash = {
        let mut hasher = DefaultHasher::new();
        for (text, color) in rich_spans {
            text.hash(&mut hasher);
            let Color(rgba) = color;
            rgba.hash(&mut hasher);
        }
        // Include bg rects in hash
        for r in bg_rects {
            r.x.to_bits().hash(&mut hasher);
            r.y.to_bits().hash(&mut hasher);
            r.color[0].to_bits().hash(&mut hasher);
            r.color[1].to_bits().hash(&mut hasher);
            r.color[2].to_bits().hash(&mut hasher);
        }
        hasher.finish()
    };

    let content_changed = frame_hash != state.last_frame_hash;
    if content_changed {
        state.last_frame_hash = frame_hash;

        let glyphon_spans: Vec<(&str, Attrs)> = rich_spans
            .iter()
            .map(|(text, color)| (text.as_str(), mono_attrs(*color)))
            .collect();

        state.cached_text_buffer.set_metrics(
            &mut state.gpu.font_system,
            Metrics::new(state.gpu.font_size, state.gpu.line_height),
        );
        state.cached_text_buffer.set_size(
            &mut state.gpu.font_system,
            Some(state.gpu.physical_width as f32),
            Some(state.gpu.physical_height as f32),
        );
        state.cached_text_buffer.set_rich_text(
            &mut state.gpu.font_system,
            glyphon_spans,
            &mono_attrs(FG),
            Shaping::Basic,
            None,
        );
        state.cached_text_buffer.shape_until_scroll(&mut state.gpu.font_system, false);
    }

    let text_areas = [TextArea {
        buffer: &state.cached_text_buffer,
        left: 0.0,
        top: 0.0,
        scale: 1.0,
        bounds: TextBounds {
            left: 0,
            top: 0,
            right: state.gpu.physical_width as i32,
            bottom: state.gpu.physical_height as i32,
        },
        default_color: FG,
        custom_glyphs: &[],
    }];

    state.gpu.render_frame(bg_rects, &text_areas);

    // Trim atlas only when content changed to avoid evicting glyphs every frame.
    if content_changed {
        state.gpu.trim_atlas();
    }
}

// ── Tab bar ───────────────────────────────────────────────────────────────────

fn render_tab_bar(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
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

// ── Pane rendering ────────────────────────────────────────────────────────────

fn render_pane_area(editor: &EditorState, cols: usize, rows: usize) -> Vec<Vec<RichSpan>> {
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
fn render_single_pane(
    pane: &Pane,
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

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
    let idx = editor.active_tab;
    let ws = &editor.tabs[idx];
    let pane = ws.panes.focused_pane();
    let buf = pane.content.as_buffer_like();
    let cursor = buf.cursor();
    let total = buf.len_lines();
    let pane_count = ws.panes.pane_count();

    // LSP status
    let lsp_status = if !ws.lsp_clients.is_empty() {
        let langs: Vec<&str> = ws.lsp_clients.keys().map(|s| s.as_str()).collect();
        format!(" LSP:{}", langs.join(","))
    } else {
        String::new()
    };

    // Diagnostics summary
    let diag_summary = {
        let uri = match &pane.content {
            PaneContent::Editor(b) => b.file_uri(),
            _ => None,
        };
        if let Some(diags) = uri.and_then(|u| ws.diagnostics.get(&u)) {
            let errors = diags.iter().filter(|d| d.severity == novim_core::lsp::DiagnosticSeverity::Error).count();
            let warnings = diags.iter().filter(|d| d.severity == novim_core::lsp::DiagnosticSeverity::Warning).count();
            if errors > 0 || warnings > 0 {
                format!(" {}E {}W", errors, warnings)
            } else { String::new() }
        } else { String::new() }
    };

    let mode_name = if let Some(reg) = editor.macros.recording {
        format!("REC @{}", reg)
    } else if editor.input_state == novim_core::input::InputState::WaitingPaneCommand {
        "CTRL+W...".to_string()
    } else if buf.is_terminal() {
        "TERMINAL".to_string()
    } else {
        editor.mode.display_name().to_string()
    };

    let pane_info = if pane_count > 1 {
        format!(" [pane {}/{}]", ws.panes.focused_id() + 1, pane_count)
    } else {
        String::new()
    };

    let left = if let Some(ref msg) = editor.status_message {
        format!(" {} │ {}{}{}", mode_name, msg, diag_summary, pane_info)
    } else {
        format!(" {}{}{}", mode_name, diag_summary, pane_info)
    };

    let right = format!(
        "{} │ {}:{} │ {}/{} ",
        lsp_status,
        cursor.line + 1,
        cursor.column + 1,
        cursor.line + 1,
        total,
    );

    let padding = cols.saturating_sub(left.len() + right.len());
    let full = format!("{}{:padding$}{}", left, "", right, padding = padding);

    // Colorize mode name
    let mode_color = match editor.mode {
        EditorMode::Normal => BLUE,
        EditorMode::Insert => GREEN,
        EditorMode::Visual | EditorMode::VisualBlock => YELLOW,
        EditorMode::Command => YELLOW,
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

fn render_command_line(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
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

fn render_search_line(editor: &EditorState, cols: usize) -> Vec<RichSpan> {
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

fn render_explorer(editor: &EditorState, cols: usize, rows: usize) -> Vec<Vec<RichSpan>> {
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

// ── Syntax highlighting ───────────────────────────────────────────────────────

fn apply_syntax_highlights(
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
        let start = span.start.min(content.len());
        let end = span.end.min(content.len());

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

fn highlight_group_color(group: HighlightGroup, theme: &SyntaxTheme) -> Color {
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
        HighlightGroup::None => return FG,
    };
    config_color_to_glyphon(config::parse_color(color_str))
}

fn config_color_to_glyphon(c: config::Color) -> Color {
    match c {
        config::Color::Black => Color::rgb(0, 0, 0),
        config::Color::Red => Color::rgb(224, 108, 117),
        config::Color::Green => Color::rgb(152, 195, 121),
        config::Color::Yellow => Color::rgb(229, 192, 123),
        config::Color::Blue => Color::rgb(97, 175, 239),
        config::Color::Magenta => Color::rgb(198, 120, 221),
        config::Color::Cyan => Color::rgb(86, 182, 194),
        config::Color::White => Color::rgb(220, 220, 220),
        config::Color::DarkGray => Color::rgb(100, 100, 100),
        config::Color::LightRed => Color::rgb(240, 140, 140),
        config::Color::LightGreen => Color::rgb(180, 220, 160),
        config::Color::LightYellow => Color::rgb(240, 210, 150),
        config::Color::LightBlue => Color::rgb(140, 200, 250),
        config::Color::LightMagenta => Color::rgb(220, 160, 240),
        config::Color::LightCyan => Color::rgb(130, 210, 220),
        config::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
        config::Color::Indexed(idx) => indexed_256_to_rgb(idx),
    }
}

/// Convert a 256-color index to approximate RGB.
fn indexed_256_to_rgb(idx: u8) -> Color {
    match idx {
        0 => Color::rgb(0, 0, 0),
        1 => Color::rgb(128, 0, 0),
        2 => Color::rgb(0, 128, 0),
        3 => Color::rgb(128, 128, 0),
        4 => Color::rgb(0, 0, 128),
        5 => Color::rgb(128, 0, 128),
        6 => Color::rgb(0, 128, 128),
        7 => Color::rgb(192, 192, 192),
        8 => Color::rgb(128, 128, 128),
        9 => Color::rgb(255, 0, 0),
        10 => Color::rgb(0, 255, 0),
        11 => Color::rgb(255, 255, 0),
        12 => Color::rgb(0, 0, 255),
        13 => Color::rgb(255, 0, 255),
        14 => Color::rgb(0, 255, 255),
        15 => Color::rgb(255, 255, 255),
        16..=231 => {
            let n = idx - 16;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            Color::rgb(r, g, b)
        }
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            Color::rgb(gray, gray, gray)
        }
    }
}

// ── Selection ─────────────────────────────────────────────────────────────────

fn highlight_with_selection(
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

    // Determine selection range on this line
    let line_start = if line_num == sel_start.line { sel_start.column } else if line_num > sel_start.line { 0 } else { return base; };
    let line_end = if line_num == sel_end.line { sel_end.column + 1 } else if line_num < sel_end.line { expanded.len() } else { return base; };

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

fn apply_search_highlight(content: &str, spans: &[RichSpan], pattern: &str) -> Vec<RichSpan> {
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

// ── Terminal cells ────────────────────────────────────────────────────────────

fn cells_to_rich_spans(cells: &[novim_core::emulator::grid::Cell]) -> Vec<RichSpan> {
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

fn cell_color_to_glyphon(c: novim_core::emulator::grid::CellColor) -> Color {
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
    }
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

fn get_diag_marker(ws: &novim_core::editor::Workspace, pane: &Pane, line_num: usize) -> Option<RichSpan> {
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

// ── Cursor ────────────────────────────────────────────────────────────────────

/// Replace the character at `target_col` with inverse colors.
/// Since glyphon doesn't support per-glyph background colors, we render
/// a solid block character (█) in CURSOR_BG color to make the cursor visible.
fn apply_cursor_to_spans(spans: &mut Vec<RichSpan>, target_col: usize) {
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

// ── Utilities ─────────────────────────────────────────────────────────────────

fn expand_tabs(line: &str, tab_width: usize) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut result = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces { result.push(' '); }
            col += spaces;
        } else {
            result.push(c);
            col += 1;
        }
    }
    result
}

fn display_col(line: &str, cursor_col: usize, tab_width: usize) -> usize {
    let mut display = 0;
    for (i, c) in line.chars().enumerate() {
        if i >= cursor_col { break; }
        if c == '\t' {
            display += tab_width - (display % tab_width);
        } else {
            display += 1;
        }
    }
    display
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        chars[..max_chars].iter().collect()
    }
}

// RichSpan Clone impl
impl Clone for RichSpan {
    fn clone(&self) -> Self {
        RichSpan { text: self.text.clone(), color: self.color }
    }
}

/// Adjust viewport_offset for all panes so the cursor stays visible.
/// This mirrors the TUI renderer's scroll logic but runs as a separate pass
/// since the GUI render functions take immutable references.
fn adjust_viewports(editor: &mut EditorState, cols: usize, rows: usize) {
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

/// Render the welcome/splash screen centered in the GPU window.
fn render_welcome_screen(state: &mut crate::WindowState, cols: usize, rows: usize) {
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

    submit_frame(state, &rich_spans, &[]);
}
