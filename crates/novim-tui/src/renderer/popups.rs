//! Popup overlays: help, plugins, buffer list, completion, file finder, hover, workspaces.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

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

fn help_line<'a>(key: &'a str, desc: &'a str, key_style: Style, desc_style: Style) -> Line<'a> {
    let padding = 20usize.saturating_sub(key.len());
    Line::from(vec![
        Span::styled(key, key_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(desc, desc_style),
    ])
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
pub(super) fn render_hover_popup(f: &mut ratatui::Frame, area: Rect, text: &str, state: &EditorState) {
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
