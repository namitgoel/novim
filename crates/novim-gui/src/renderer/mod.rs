//! GPU renderer — converts EditorState into glyphon text areas.
//!
//! This mirrors the TUI renderer logic but outputs `(text, Attrs)` spans
//! that glyphon renders with per-character syntax colors.

mod theme;
mod pane;
mod popups;
mod styling;

use glyphon::{Color, Metrics, Shaping, TextArea, TextBounds};
use novim_types::EditorMode;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

use theme::{RichSpan, FG, DIM, mono_attrs};
use pane::*;
use popups::apply_popup_overlays;

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

        let glyphon_spans: Vec<(&str, glyphon::Attrs)> = rich_spans
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
