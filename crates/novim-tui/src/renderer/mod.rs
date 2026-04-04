//! Ratatui-based rendering for the editor UI.

mod pane;
mod popups;
mod styling;
mod util;

use novim_core::config;
use novim_core::welcome;
use novim_types::EditorMode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use super::EditorState;

use pane::{render_single_pane, render_explorer, render_outline_sidebar};
use popups::{
    render_help_popup, render_plugin_popup, render_buffer_list, render_completion_popup,
    render_file_finder, render_hover_popup, render_workspace_list,
    render_symbol_list, render_floating_window, render_command_window,
};
use styling::config_color_to_ratatui;
use util::TAB_COLORS;

/// Render the full editor UI: panes + status bar (+ command line if active).
/// True when the editor has a single focused terminal pane and no overlays.
pub fn render(f: &mut ratatui::Frame, state: &mut EditorState) {
    let size = f.area();
    f.render_widget(Clear, size);

    // Welcome screen: centered logo + shortcuts, full screen.
    if state.show_welcome {
        render_welcome(f, size);
        return;
    }

    // Always render with chrome (status bar, borders, etc.)
    // Pure terminal mode is disabled — novim always shows its own UI.

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

    // Breadcrumb bar (shown when outline is visible)
    let (_breadcrumb_area, content_area) = if state.outline.visible && !state.outline.breadcrumb.is_empty() {
        let vsplit = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // breadcrumb
                Constraint::Min(1),     // panes
            ])
            .split(main_chunk);
        // Render breadcrumb
        let crumb = format!(" {} ", state.outline.breadcrumb);
        let widget = Paragraph::new(Line::from(vec![
            Span::styled(crumb, Style::default().fg(Color::Indexed(252)).bg(Color::Indexed(236))),
        ])).style(Style::default().bg(Color::Indexed(236)));
        f.render_widget(widget, vsplit[0]);
        (Some(vsplit[0]), vsplit[1])
    } else {
        (None, main_chunk)
    };

    // Split horizontally if explorer or outline sidebar is open
    let ws = &state.tabs[state.active_tab];
    let has_explorer = ws.explorer.is_some();
    let has_outline = state.outline.visible && !state.outline.symbols.is_empty();

    let pane_area = if has_explorer || has_outline {
        let mut h_constraints = Vec::new();
        if has_explorer {
            h_constraints.push(Constraint::Length(30));
        }
        h_constraints.push(Constraint::Min(1)); // panes
        if has_outline {
            h_constraints.push(Constraint::Length(28)); // outline sidebar
        }
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(h_constraints)
            .split(content_area);

        let mut idx = 0;
        if has_explorer {
            render_explorer(f, hsplit[idx], state);
            idx += 1;
        }
        let pane_rect = hsplit[idx];
        idx += 1;
        if has_outline {
            render_outline_sidebar(f, hsplit[idx], state);
        }
        pane_rect
    } else {
        content_area
    };

    render_panes(f, pane_area, state, false);
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

    // Plugin popup
    if let Some(popup) = &state.plugin_popup {
        render_plugin_popup(f, size, &popup.title, &popup.lines, popup.scroll, popup.selected, popup.on_select.is_some(), popup.width, popup.height);
    }

    // Symbol list popup
    if state.symbol_list.visible {
        render_symbol_list(f, size, state);
    }

    // Floating windows (render last, on top of everything)
    for fw in &state.floating_windows {
        render_floating_window(f, size, fw);
    }

    // Command window
    if state.command_window.visible {
        render_command_window(f, size, state);
    }
}

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

fn render_panes(f: &mut ratatui::Frame, area: Rect, state: &mut EditorState, borderless: bool) {
    let idx = state.active_tab;
    let focused_id = state.tabs[idx].panes.focused_id();
    let ln_mode = state.line_number_mode;
    let focused_color = config_color_to_ratatui(config::parse_color(&state.config.theme.focused_border));
    let unfocused_color = config_color_to_ratatui(config::parse_color(&state.config.theme.unfocused_border));
    let syntax_theme = &state.config.syntax_theme;
    let tab_width = state.config.editor.tab_width;
    let word_wrap = state.config.editor.word_wrap;
    let search_pattern = state.search.pattern.as_deref();
    let show_minimap = state.config.editor.minimap;
    let minimap_width = state.config.editor.minimap_width;
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
            let is_editor = !pane.content.as_buffer_like().is_terminal();
            let minimap_w = if show_minimap && is_editor && ratatui_rect.width > 30 {
                minimap_width as u16
            } else {
                0
            };
            let pane_rect = Rect::new(ratatui_rect.x, ratatui_rect.y, ratatui_rect.width.saturating_sub(minimap_w), ratatui_rect.height);
            render_single_pane(f, pane_rect, pane, is_focused, ln_mode, border_color, diags, search_pattern, syntax_theme, tab_width, word_wrap, borderless);
            if minimap_w > 0 {
                let minimap_rect = Rect::new(pane_rect.x + pane_rect.width, ratatui_rect.y, minimap_w, ratatui_rect.height);
                pane::render_minimap(f, minimap_rect, pane);
            }
        }
    }
}

fn render_status_bar(f: &mut ratatui::Frame, area: Rect, state: &mut EditorState) {
    let info = state.status_bar_info();
    let sb_config = &state.config.status_bar;

    let left = info.format_left(&sb_config.left);
    let right = info.format_right(&sb_config.right);

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

/// Render the welcome/splash screen centered on the terminal.
fn render_welcome(f: &mut ratatui::Frame, area: Rect) {
    let wlines = welcome::welcome_lines();
    let content_height = wlines.len();
    let start_y = area.height.saturating_sub(content_height as u16) / 2;

    let mut lines: Vec<Line> = Vec::new();
    // Pad top
    for _ in 0..start_y {
        lines.push(Line::from(""));
    }

    for wl in &wlines {
        let max_visual = wlines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
        let pad = area.width as usize / 2 - max_visual.min(area.width as usize) / 2;
        let padding = " ".repeat(pad);

        let styled = match wl.kind {
            "logo" => Span::styled(
                format!("{}{}", padding, wl.text),
                Style::default().fg(Color::Indexed(75)).add_modifier(Modifier::BOLD),
            ),
            "version" => Span::styled(
                format!("{}{}", padding, wl.text),
                Style::default().fg(Color::Indexed(243)),
            ),
            "shortcut" => {
                // Split "  key   description" into two styled spans
                let text = &wl.text;
                if let Some(pos) = text.find("   ") {
                    let key_part = &text[..pos];
                    let desc_part = &text[pos + 3..];
                    lines.push(Line::from(vec![
                        Span::raw(padding.clone()),
                        Span::styled(
                            key_part.to_string(),
                            Style::default().fg(Color::Indexed(75)).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("   {}", desc_part),
                            Style::default().fg(Color::Indexed(252)),
                        ),
                    ]));
                    continue;
                }
                Span::styled(
                    format!("{}{}", padding, wl.text),
                    Style::default().fg(Color::Indexed(252)),
                )
            }
            _ => Span::raw(format!("{}{}", padding, wl.text)),
        };
        lines.push(Line::from(styled));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
