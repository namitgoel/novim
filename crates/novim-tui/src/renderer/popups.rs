//! Popup overlays: help, plugins, buffer list, completion, file finder, hover, workspaces.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use novim_core::help::{HelpEntry, help_entries};
use super::styling::apply_highlights;
use super::EditorState;

/// Render centered help popup overlay.
pub(super) fn render_help_popup(f: &mut ratatui::Frame, area: Rect, scroll: usize) {
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

    let all_lines: Vec<Line> = help_entries().iter().map(|entry| match entry {
        HelpEntry::Section(title) => Line::from(Span::styled(format!("  {}", title), h)),
        HelpEntry::Shortcut { key, desc } => {
            let key_str = format!("  {}", key);
            let padding = 20usize.saturating_sub(key_str.len());
            Line::from(vec![
                Span::styled(key_str, k),
                Span::raw(" ".repeat(padding)),
                Span::styled(desc.to_string(), d),
            ])
        }
        HelpEntry::Blank => Line::from(""),
        HelpEntry::Footer(text) => Line::from(Span::styled(format!("  {}", text), dim)),
    }).collect();

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

/// Render a plugin popup overlay.
pub(super) fn render_plugin_popup(f: &mut ratatui::Frame, area: Rect, title: &str, lines: &[String], scroll: usize, selected: usize, selectable: bool, custom_width: Option<u16>, custom_height: Option<u16>) {
    let auto_width = lines.iter().map(|l| l.len()).max().unwrap_or(20).max(title.len() + 4) as u16 + 4;
    let auto_height = lines.len() as u16 + 2;
    let popup_width = custom_width.unwrap_or(auto_width).clamp(10, area.width.saturating_sub(4));
    let popup_height = custom_height.unwrap_or(auto_height).clamp(4, area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let visible_height = popup_height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);
    let visible_lines: Vec<Line> = lines.iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, l)| {
            if selectable && i == selected {
                Line::from(Span::styled(
                    format!(" > {} ", l),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ))
            } else {
                Line::from(Span::styled(format!("   {} ", l), Style::default().fg(Color::White)))
            }
        })
        .collect();

    let title_str = if lines.len() > visible_height {
        format!(" {} ({}/{}) ", title, scroll + 1, max_scroll + 1)
    } else {
        format!(" {} ", title)
    };

    let popup = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title_str)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );

    f.render_widget(popup, popup_area);
}

/// Render buffer list popup.
pub(super) fn render_buffer_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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
pub(super) fn render_completion_popup(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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
pub(super) fn render_file_finder(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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

pub(super) fn render_finder_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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
        let char_count = result.display.chars().count();
        let display = if char_count > max_len {
            let skip = char_count.saturating_sub(max_len - 3);
            let tail: String = result.display.chars().skip(skip).collect();
            format!("...{}", tail)
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

pub(super) fn render_finder_preview(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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
            let content: String = line.chars().take(max_width).collect();
            let content = content.as_str();

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
pub(super) fn render_hover_popup(f: &mut ratatui::Frame, area: Rect, text: &str, state: &EditorState) {
    let lines: Vec<&str> = text.lines().collect();
    let max_line_width = lines.iter().map(|l| l.len()).max().unwrap_or(20);

    let popup_width = (max_line_width as u16 + 4).min(area.width.saturating_sub(10)).max(20);
    let popup_height = (lines.len() as u16 + 2).min(15).min(area.height.saturating_sub(4));

    // Position above cursor (use screen-relative coordinates)
    let pane = state.tabs[state.active_tab].panes.focused_pane();
    let cursor = pane.content.as_buffer_like().cursor();
    let screen_line = cursor.line.saturating_sub(pane.viewport_offset) as u16;
    let x = (6 + cursor.column as u16).min(area.width.saturating_sub(popup_width));
    let y = if screen_line > popup_height + 2 {
        area.y + screen_line - popup_height
    } else {
        (area.y + screen_line + 2).min(area.y + area.height.saturating_sub(popup_height))
    };
    let popup_area = Rect::new(
        x.min(area.x + area.width.saturating_sub(popup_width)),
        y.min(area.y + area.height.saturating_sub(popup_height)),
        popup_width,
        popup_height,
    );

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
pub(super) fn render_workspace_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
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

/// Render the symbol list popup (Ctrl+T / :symbols).
pub(super) fn render_symbol_list(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = (area.height * 3 / 4).max(10).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let sl = &state.symbol_list;
    let visible_height = popup_height.saturating_sub(4) as usize;
    let scroll = if sl.selected >= visible_height { sl.selected - visible_height + 1 } else { 0 };

    let mut lines = Vec::new();
    // Query line
    lines.push(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Yellow)),
        Span::raw(&sl.query),
    ]));

    for (i, &idx) in sl.filtered.iter().enumerate().skip(scroll).take(visible_height) {
        let sym = &sl.symbols[idx];
        let is_selected = i == sl.selected;
        let style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default()
        };
        let kind_style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Cyan)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>6} ", sym.kind.label()), kind_style),
            Span::styled(format!("{:<30} :{}", sym.name, sym.line + 1), style),
        ]));
    }

    let title = format!(" Symbols ({}) ", sl.filtered.len());
    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );
    f.render_widget(popup, popup_area);
}

/// Render a floating window.
pub(super) fn render_floating_window(
    f: &mut ratatui::Frame,
    area: Rect,
    fw: &novim_core::editor::FloatingWindow,
) {
    let popup_width = fw.width.min(area.width.saturating_sub(4));
    let popup_height = fw.height.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let visible_height = popup_height.saturating_sub(2) as usize;
    let lines: Vec<Line> = fw.lines.iter()
        .skip(fw.scroll)
        .take(visible_height)
        .map(|l| Line::from(Span::raw(l.as_str())))
        .collect();

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", fw.title))
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );
    f.render_widget(popup, popup_area);
}

/// Render the command history window (q:).
pub(super) fn render_command_window(f: &mut ratatui::Frame, area: Rect, state: &EditorState) {
    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = (area.height / 2).max(10).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let visible_height = popup_height.saturating_sub(2) as usize;
    let selected = state.command_window.selected;
    let scroll = if selected >= visible_height { selected - visible_height + 1 } else { 0 };

    let lines: Vec<Line> = state.command_history.iter().enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, cmd)| {
            let style = if i == selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!(" :{}", cmd), style))
        })
        .collect();

    let title = format!(" Command History ({}) ", state.command_history.len());
    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
    );
    f.render_widget(popup, popup_area);
}
