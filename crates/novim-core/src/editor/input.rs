//! Input handling, plugin dispatch, and buffer snapshot methods.

use crossterm::event::{MouseEvent, MouseEventKind, MouseButton};

use crate::input::{parse_ex_command, EditorCommand};
use crate::pane::PaneContent;
use novim_types::{EditorMode, Selection};

use super::{EditorState, ExecOutcome};

impl EditorState {
    /// Handle a mouse event. Frontend-agnostic — takes crossterm MouseEvent.
    pub fn handle_mouse(&mut self, mouse: MouseEvent, screen_area: novim_types::Rect) {
        let idx = self.active_tab;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let click_x = mouse.column;
                let click_y = mouse.row;

                let pane_area = novim_types::Rect::new(
                    screen_area.x,
                    screen_area.y,
                    screen_area.width,
                    screen_area.height.saturating_sub(1),
                );
                let layouts = self.tabs[idx].panes.layout(pane_area);

                for (pane_id, rect) in &layouts {
                    if click_x >= rect.x
                        && click_x < rect.x + rect.width
                        && click_y >= rect.y
                        && click_y < rect.y + rect.height
                    {
                        self.tabs[idx].panes.try_set_focus(*pane_id);

                        let is_terminal = self.tabs[idx].panes.focused_pane().content.as_buffer_like().is_terminal();
                        let show_minimap = self.config.editor.minimap && !is_terminal && rect.width > 30;
                        let minimap_w = if show_minimap { self.config.editor.minimap_width as u16 } else { 0 };
                        let pane_right = rect.x + rect.width.saturating_sub(minimap_w);

                        // Check if click is in minimap area
                        if show_minimap && click_x >= pane_right {
                            if let Some(pane) = self.tabs[idx].panes.get_pane_mut(*pane_id) {
                                let total_lines = pane.content.as_buffer_like().len_lines().max(1);
                                let height = rect.height as usize;
                                let local_y = click_y.saturating_sub(rect.y) as usize;
                                let scale = (total_lines as f64 / height as f64).max(1.0);
                                let target_line = (local_y as f64 * scale) as usize;
                                let target_line = target_line.min(total_lines.saturating_sub(1));
                                pane.content.as_buffer_like_mut().set_cursor_pos(
                                    novim_types::Position::new(target_line, 0),
                                );
                                // Center viewport on target
                                let half = height / 2;
                                pane.viewport_offset = target_line.saturating_sub(half);
                            }
                            break;
                        }

                        let border_offset = 1u16;
                        let col_offset = if is_terminal { 1 } else { 6 };

                        let local_y = click_y.saturating_sub(rect.y + border_offset);
                        let local_x = click_x.saturating_sub(rect.x + col_offset);

                        if let Some(pane) = self.tabs[idx].panes.get_pane_mut(*pane_id) {
                            let line = pane.viewport_offset + local_y as usize;
                            let col = local_x as usize;

                            // Check for OSC 8 hyperlink on terminal cells
                            if is_terminal {
                                if let Some(cells) = pane.content.as_buffer_like().get_styled_cells(local_y as usize) {
                                    if let Some(cell) = cells.get(col) {
                                        if let Some(ref url) = cell.hyperlink {
                                            crate::url::open_url(url);
                                            break;
                                        }
                                    }
                                }
                            }

                            pane.content.as_buffer_like_mut().set_cursor_pos(
                                novim_types::Position::new(line, col),
                            );
                        }
                        break;
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                let n = self.config.editor.mouse_scroll_lines;
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    pane.viewport_offset = pane.viewport_offset.saturating_sub(n);
                }
            }
            MouseEventKind::ScrollDown => {
                let n = self.config.editor.mouse_scroll_lines;
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let max = pane.content.as_buffer_like().len_lines().saturating_sub(1);
                    pane.viewport_offset = (pane.viewport_offset + n).min(max);
                }
            }
            _ => {}
        }
    }

    /// Look up a plugin keymap and execute it if found. Returns true if handled.
    pub fn try_plugin_keymap(&mut self, mode: &str, key_str: &str, screen_area: novim_types::Rect) -> bool {
        let entry = self.plugins.keymaps.lookup(mode, key_str);
        let action = match entry {
            Some(e) => e.action.clone(),
            None => return false,
        };
        match action {
            crate::plugin::KeymapAction::Command(cmd) => {
                let parsed = parse_ex_command(&cmd);
                let _ = self.execute(parsed, screen_area);
            }
            crate::plugin::KeymapAction::LuaCallback { plugin_id: _, callback_key } => {
                let snapshot = self.make_buffer_snapshot();
                let event = crate::plugin::EditorEvent::CommandExecuted {
                    command: format!("__keymap:{}", callback_key),
                };
                let actions = self.plugins.dispatch(&event, &snapshot);
                self.run_plugin_actions(actions, screen_area);
            }
        }
        true
    }

    /// Handle Enter on a selectable plugin popup. Calls the on_select callback.
    pub fn handle_popup_select(&mut self, screen_area: novim_types::Rect) {
        let popup = match self.plugin_popup.take() {
            Some(p) => p,
            None => return,
        };
        let (_plugin_id, callback_key) = match popup.on_select {
            Some(cb) => cb,
            None => return, // display-only popup, Enter does nothing
        };
        let selected_index = popup.selected;
        let selected_text = popup.lines.get(selected_index).cloned().unwrap_or_default();

        // Dispatch as __popup_select:<callback_key>:<index>:<text>
        let snapshot = self.make_buffer_snapshot();
        let event = crate::plugin::EditorEvent::CommandExecuted {
            command: format!("__popup_select:{}:{}:{}", callback_key, selected_index, selected_text),
        };
        let actions = self.plugins.dispatch(&event, &snapshot);
        self.run_plugin_actions(actions, screen_area);
    }

    /// Run a callback with the LSP client for the focused buffer's language.
    /// Navigate the jump list forward or backward.
    pub(super) fn navigate_jump_list(&mut self, forward: bool) -> Result<ExecOutcome, crate::error::NovimError> {
        let can_move = if forward {
            self.jump_index + 1 < self.jump_list.len()
        } else {
            self.jump_index > 0
        };
        if can_move {
            if forward { self.jump_index += 1; } else { self.jump_index -= 1; }
            if let Some((path, pos)) = self.jump_list.get(self.jump_index).cloned() {
                let current_name = self.focused_buf().display_name();
                if path != current_name {
                    let _ = self.handle_edit_file(&path);
                }
                self.focused_buf_mut().set_cursor_pos(pos);
            }
        } else {
            let msg = if forward { "Already at newest jump" } else { "Already at oldest jump" };
            self.status_message = Some(msg.to_string());
        }
        Ok(ExecOutcome::Continue)
    }

    /// Record the current position in the jump list before a big jump.
    pub fn push_jump(&mut self) {
        let path = self.focused_buf().display_name();
        let pos = self.focused_buf().cursor();
        // Truncate any forward history
        self.jump_list.truncate(self.jump_index);
        self.jump_list.push((path, pos));
        self.jump_index = self.jump_list.len();
        // Cap at 100 entries
        if self.jump_list.len() > 100 {
            self.jump_list.remove(0);
            self.jump_index = self.jump_list.len();
        }
    }

    /// Store text in a register. Also updates system clipboard for unnamed/+ register.
    pub(super) fn yank_to_register(&mut self, text: &str) {
        let reg = self.pending_register.take().unwrap_or('"');
        self.registers.insert(reg, text.to_string());
        // Unnamed register always gets a copy
        if reg != '"' {
            self.registers.insert('"', text.to_string());
        }
        // System clipboard for unnamed or +
        if reg == '"' || reg == '+' {
            super::types::set_system_clipboard(text);
        }
    }

    /// Read text from a register. Falls back to system clipboard for unnamed/+.
    pub(super) fn paste_from_register(&mut self) -> String {
        let reg = self.pending_register.take().unwrap_or('"');
        if reg == '+' || reg == '"' {
            // Prefer system clipboard, fall back to register
            super::types::get_system_clipboard()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.registers.get(&reg).cloned().unwrap_or_default())
        } else {
            self.registers.get(&reg).cloned().unwrap_or_default()
        }
    }

    /// Build a read-only snapshot of the focused buffer for plugin dispatch.
    pub fn make_buffer_snapshot(&self) -> crate::plugin::BufferSnapshot {
        let buf = self.focused_buf();
        let cursor = buf.cursor();
        // Get full file path from the Buffer (not display_name which is just the filename)
        let full_path = {
            let pane = self.tabs[self.active_tab].panes.focused_pane();
            match &pane.content {
                crate::pane::PaneContent::Editor(b) => b.file_path_str().map(|s| s.to_string()),
                _ => None,
            }
        };
        let sel = buf.selection().map(|s| {
            let (start, end) = s.ordered();
            (start.line, start.column, end.line, end.column)
        });
        // Extract text and version from Buffer (uses cached_text for O(1) when clean)
        let (text, version) = {
            let pane = self.tabs[self.active_tab].panes.focused_pane();
            match &pane.content {
                crate::pane::PaneContent::Editor(b) => (Some(b.full_text()), Some(b.version())),
                _ => (None, None),
            }
        };
        let line_count = buf.len_lines();
        crate::plugin::BufferSnapshot {
            lines: Vec::new(), // Lazy: populated on-demand by Lua bridge via get_lines()
            line_count,
            cursor_line: cursor.line,
            cursor_col: cursor.column,
            path: full_path,
            is_dirty: buf.is_dirty(),
            mode: self.mode.display_name().to_string(),
            selection: sel,
            selected_text: buf.selected_text(),
            tab_width: self.config.editor.tab_width,
            expand_tab: self.config.editor.expand_tab,
            auto_indent: self.config.editor.auto_indent,
            word_wrap: self.config.editor.word_wrap,
            line_numbers: self.config.editor.line_numbers.clone(),
            pane_count: self.tabs[self.active_tab].panes.pane_count(),
            text,
            version,
        }
    }

    /// Map a command to the editor events it should emit (called before execution
    /// so we can capture pre-state like file paths).
    pub(super) fn events_for_command(cmd: &EditorCommand, state: &EditorState) -> Vec<crate::plugin::EditorEvent> {
        use crate::plugin::EditorEvent;
        let path = || -> String {
            state.focused_buf().display_name()
        };
        match cmd {
            EditorCommand::Save | EditorCommand::SaveAndQuit => {
                vec![EditorEvent::BufWrite { path: path() }]
            }
            EditorCommand::EditFile(p) => {
                vec![EditorEvent::BufOpen { path: p.clone() }]
            }
            EditorCommand::InsertChar(_)
            | EditorCommand::InsertTab
            | EditorCommand::InsertNewline
            | EditorCommand::DeleteCharBefore
            | EditorCommand::Paste
            | EditorCommand::PasteBefore
            | EditorCommand::DeleteLines(_)
            | EditorCommand::DeleteMotion(..)
            | EditorCommand::ChangeMotion(..)
            | EditorCommand::ChangeLines(_)
            | EditorCommand::ReplaceAll(..)
            | EditorCommand::ReplaceConfirmYes
            | EditorCommand::ReplaceConfirmAll
            | EditorCommand::Undo
            | EditorCommand::Redo
            | EditorCommand::DeleteSelection
            | EditorCommand::DeleteTextObject(..)
            | EditorCommand::ChangeTextObject(..)
            | EditorCommand::CompletionAccept
            | EditorCommand::ReplaceChar(_)
            | EditorCommand::DeleteCharForward
            | EditorCommand::OpenLineBelow
            | EditorCommand::OpenLineAbove
            | EditorCommand::ChangeToEnd
            | EditorCommand::DeleteToEnd
            | EditorCommand::SubstituteLine
            | EditorCommand::JoinLines(_)
            | EditorCommand::Indent(_)
            | EditorCommand::Dedent(_)
            | EditorCommand::ToggleCase
            | EditorCommand::ReplaceInsertChar(_)
            | EditorCommand::SortLines
            | EditorCommand::AutoIndent => {
                vec![EditorEvent::TextChanged { path: path() }]
            }
            EditorCommand::CommandExecute => {
                vec![EditorEvent::CommandExecuted {
                    command: state.command_buffer.clone(),
                }]
            }
            _ => vec![],
        }
    }

    /// Execute actions returned by plugins.
    pub fn run_plugin_actions(&mut self, actions: Vec<crate::plugin::PluginAction>, screen_area: novim_types::Rect) {
        use crate::plugin::PluginAction;
        for action in actions {
            match action {
                PluginAction::ExecCommand(cmd_str) => {
                    let parsed = parse_ex_command(&cmd_str);
                    let _ = self.execute(parsed, screen_area);
                }
                PluginAction::SetLines { start, end, lines } => {
                    let buf = self.focused_buf_mut();
                    // Delete lines [start..end], then insert new lines at start
                    let delete_count = end.saturating_sub(start);
                    for _ in 0..delete_count {
                        buf.set_cursor_pos(novim_types::Position::new(start, 0));
                        buf.delete_lines(1);
                    }
                    buf.set_cursor_pos(novim_types::Position::new(start, 0));
                    for line in &lines {
                        for c in line.chars() {
                            buf.insert_char(c);
                        }
                        buf.insert_char('\n');
                    }
                    buf.break_undo_group();
                }
                PluginAction::InsertText { line, col, text } => {
                    let buf = self.focused_buf_mut();
                    buf.set_cursor_pos(novim_types::Position::new(line, col));
                    for c in text.chars() {
                        buf.insert_char(c);
                    }
                    buf.break_undo_group();
                }
                PluginAction::SetCursor { line, col } => {
                    self.focused_buf_mut().set_cursor_pos(
                        novim_types::Position::new(line, col),
                    );
                }
                PluginAction::SetStatus(msg) => {
                    self.status_message = Some(msg);
                }
                PluginAction::RegisterKeymap { mode, key, action } => {
                    self.plugins.keymaps.register(&mode, &key, "lua", action);
                }
                PluginAction::SetSelection { start_line, start_col, end_line, end_col } => {
                    let anchor = novim_types::Position::new(start_line, start_col);
                    let head = novim_types::Position::new(end_line, end_col);
                    self.focused_buf_mut().set_selection(Some(Selection::new(anchor, head)));
                    self.mode = EditorMode::Visual;
                }
                PluginAction::ClearSelection => {
                    self.focused_buf_mut().set_selection(None);
                    if self.mode.is_visual() {
                        self.mode = EditorMode::Normal;
                    }
                }
                PluginAction::ShowPopup { title, lines, width, height, on_select } => {
                    self.plugin_popup = Some(super::PluginPopup {
                        title, lines, scroll: 0, selected: 0, width, height, on_select,
                    });
                }
                PluginAction::OpenFloat { title, lines, width, height } => {
                    self.floating_windows.push(super::FloatingWindow {
                        title, lines, width, height, scroll: 0, selected: 0,
                    });
                }
                PluginAction::CloseFloat => {
                    self.floating_windows.pop();
                }
                PluginAction::SetGutterSigns(signs) => {
                    let idx = self.active_tab;
                    let pane = self.tabs[idx].panes.focused_pane_mut();
                    if let crate::pane::PaneContent::Editor(buf) = &mut pane.content {
                        buf.git_signs = signs;
                    }
                }
                PluginAction::EmitEvent { name, data } => {
                    let snapshot = self.make_buffer_snapshot();
                    let event = crate::plugin::EditorEvent::Custom { name, data };
                    let actions = self.plugins.dispatch(&event, &snapshot);
                    self.run_plugin_actions(actions, screen_area);
                }
                // LSP plugin actions
                PluginAction::SetDiagnostics { uri, diagnostics } => {
                    let idx = self.active_tab;
                    self.tabs[idx].diagnostics.insert(uri, diagnostics);
                }
                PluginAction::ShowCompletions { items } => {
                    if !items.is_empty() {
                        self.completion.items = items;
                        self.completion.selected = 0;
                        self.completion.visible = true;
                    }
                }
                PluginAction::ShowHoverText { text } => {
                    self.hover_text = Some(text);
                }
                PluginAction::GotoLocation { file, line, col } => {
                    self.push_jump();
                    let _ = self.handle_edit_file(&file);
                    self.focused_buf_mut().set_cursor_pos(
                        novim_types::Position::new(line as usize, col as usize),
                    );
                }
                PluginAction::SetLspStatus { lang: _, message } => {
                    let idx = self.active_tab;
                    self.tabs[idx].lsp_status = message;
                }
            }
        }
    }

    /// Check if any open files have been modified externally.
    /// Auto-reloads clean buffers; warns for dirty ones.
    pub fn check_external_changes(&mut self) {
        for ws in &mut self.tabs {
            ws.panes.for_each_pane_mut(|pane| {
                if let PaneContent::Editor(buf) = &mut pane.content {
                    if let (Some(path), Some(last_mod)) = (buf.file_path_str().map(|s| s.to_string()), buf.last_modified) {
                        if let Ok(meta) = std::fs::metadata(&path) {
                            if let Ok(current_mod) = meta.modified() {
                                if current_mod > last_mod {
                                    if !crate::buffer::PaneDisplay::is_dirty(buf) {
                                        buf.reload_from_file();
                                    }
                                    // Update mtime even for dirty buffers to avoid repeated warnings
                                    buf.last_modified = Some(current_mod);
                                }
                            }
                        }
                    }
                }
            });
        }
    }
}
