//! All execute_* handler methods and handle_* methods for EditorState.

use std::path::PathBuf;
use std::sync::Arc;

use crate::buffer::Buffer;
use crate::error::NovimError;
use crate::explorer::Explorer;
use crate::finder;
use crate::highlight;
use crate::input::{key_to_command, parse_ex_command, EditorCommand};
use crate::pane::{PaneContent, SplitDirection};
use crate::session;
use novim_types::{EditorMode, Selection};

use super::{EditorState, ExecOutcome, LineNumberMode, Workspace, resolve_path};

impl EditorState {
    pub(super) fn execute_navigation(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::MoveCursor(dir) => {
                self.focused_buf_mut().move_cursor(dir);
                if self.mode.is_visual() {
                    let cursor = self.focused_buf().cursor();
                    if let Some(sel) = self.focused_buf().selection() {
                        self.focused_buf_mut()
                            .set_selection(Some(Selection::new(sel.anchor, cursor)));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::MoveCursorN(dir, n) => {
                let buf = self.focused_buf_mut();
                for _ in 0..n {
                    buf.move_cursor(dir);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::AddCursorAbove => {
                self.focused_buf_mut().add_cursor_above();
                let n = self.focused_buf().secondary_cursors().len();
                self.status_message = Some(format!("{} cursors", n + 1));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::AddCursorBelow => {
                self.focused_buf_mut().add_cursor_below();
                let n = self.focused_buf().secondary_cursors().len();
                self.status_message = Some(format!("{} cursors", n + 1));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ClearSecondaryCursors => {
                self.focused_buf_mut().clear_secondary_cursors();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FindChar(c, forward) => {
                self.last_find_char = Some((c, forward, true));
                let cursor = self.focused_buf().cursor();
                let line = self.focused_buf().get_line(cursor.line).unwrap_or_default();
                let chars: Vec<char> = line.chars().collect();
                let new_col = if forward {
                    chars.iter().enumerate().skip(cursor.column + 1).find(|(_, &ch)| ch == c).map(|(i, _)| i)
                } else {
                    chars.iter().enumerate().take(cursor.column).rev().find(|(_, &ch)| ch == c).map(|(i, _)| i)
                };
                if let Some(col) = new_col {
                    self.focused_buf_mut().set_cursor_pos(novim_types::Position::new(cursor.line, col));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::TillChar(c, forward) => {
                self.last_find_char = Some((c, forward, false));
                let cursor = self.focused_buf().cursor();
                let line = self.focused_buf().get_line(cursor.line).unwrap_or_default();
                let chars: Vec<char> = line.chars().collect();
                let new_col = if forward {
                    chars.iter().enumerate().skip(cursor.column + 1).find(|(_, &ch)| ch == c).map(|(i, _)| i.saturating_sub(1).max(cursor.column + 1))
                } else {
                    chars.iter().enumerate().take(cursor.column).rev().find(|(_, &ch)| ch == c).map(|(i, _)| i + 1)
                };
                if let Some(col) = new_col {
                    self.focused_buf_mut().set_cursor_pos(novim_types::Position::new(cursor.line, col));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::RepeatFindChar => {
                if let Some((c, forward, inclusive)) = self.last_find_char {
                    let cmd = if inclusive {
                        EditorCommand::FindChar(c, forward)
                    } else {
                        EditorCommand::TillChar(c, forward)
                    };
                    return self.execute_navigation(cmd);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::RepeatFindCharReverse => {
                if let Some((c, forward, inclusive)) = self.last_find_char {
                    let cmd = if inclusive {
                        EditorCommand::FindChar(c, !forward)
                    } else {
                        EditorCommand::TillChar(c, !forward)
                    };
                    return self.execute_navigation(cmd);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::MatchBracket => {
                if let Some(pos) = self.focused_buf().find_matching_bracket() {
                    self.focused_buf_mut().set_cursor_pos(pos);
                }
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_fold(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::ToggleFold => {
                let line = self.focused_buf().cursor().line;
                if self.focused_buf_mut().toggle_fold(line) {
                    self.status_message = Some("Fold toggled".to_string());
                } else {
                    self.status_message = Some("No fold at cursor".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FoldAll => {
                let tw = self.config.editor.tab_width;
                self.focused_buf_mut().recompute_folds(tw);
                self.focused_buf_mut().fold_all();
                self.status_message = Some("All folds collapsed".to_string());
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::UnfoldAll => {
                self.focused_buf_mut().unfold_all();
                self.status_message = Some("All folds expanded".to_string());
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_visual(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::EnterVisual => {
                let cursor = self.focused_buf().cursor();
                self.focused_buf_mut()
                    .set_selection(Some(Selection::new(cursor, cursor)));
                self.mode = EditorMode::Visual;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::EnterVisualBlock => {
                let cursor = self.focused_buf().cursor();
                self.focused_buf_mut()
                    .set_selection(Some(Selection::new(cursor, cursor)));
                self.mode = EditorMode::VisualBlock;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteSelection => {
                if let Some(text) = self.focused_buf_mut().delete_selection() {
                    self.yank_to_register(&text);
                    self.focused_buf_mut().break_undo_group();
                }
                self.mode = EditorMode::Normal;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::YankSelection => {
                if let Some(text) = self.focused_buf().selected_text() {
                    self.yank_to_register(&text);
                    self.status_message = Some("Yanked".to_string());
                }
                self.focused_buf_mut().set_selection(None);
                self.mode = EditorMode::Normal;
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_editing(
        &mut self,
        cmd: EditorCommand,
        _screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::Paste => {
                let clip = self.paste_from_register();
                if !clip.is_empty() {
                    let buf = self.focused_buf_mut();
                    for c in clip.chars() {
                        buf.insert_char(c);
                    }
                    buf.break_undo_group();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PasteBefore => {
                let clip = self.paste_from_register();
                if !clip.is_empty() {
                    let buf = self.focused_buf_mut();
                    let cursor_before = buf.cursor();
                    for c in clip.chars() {
                        buf.insert_char(c);
                    }
                    buf.break_undo_group();
                    // Move cursor back to the original position
                    buf.set_cursor_pos(cursor_before);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SwitchMode(mode) => {
                self.focused_buf_mut().break_undo_group();
                if self.mode.is_visual() && !mode.is_visual() {
                    // Save the selection for gv before clearing
                    if let Some(sel) = self.focused_buf().selection() {
                        self.last_visual_selection = Some(sel);
                    }
                    self.focused_buf_mut().set_selection(None);
                }
                if mode == EditorMode::Normal {
                    self.focused_buf_mut().clear_secondary_cursors();
                }
                if mode == EditorMode::Command {
                    self.command_buffer.clear();
                }
                self.mode = mode;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::InsertChar(c) => {
                self.focused_buf_mut().insert_char(c);
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ReplaceInsertChar(c) => {
                // Replace mode: overwrite char at cursor instead of inserting
                if let crate::pane::PaneContent::Editor(buf) = &mut self.tabs[idx].panes.focused_pane_mut().content {
                    buf.replace_insert_char(c);
                }
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::InsertTab => {
                let tw = self.config.editor.tab_width;
                let et = self.config.editor.expand_tab;
                self.focused_buf_mut().insert_tab(tw, et);
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteCharBefore => {
                self.focused_buf_mut().delete_char_before_cursor();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::InsertNewline => {
                let ai = self.config.editor.auto_indent;
                self.focused_buf_mut().insert_newline_with_indent(ai);
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Undo => {
                let msg = self.focused_buf_mut().undo();
                self.status_message = msg;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Redo => {
                let msg = self.focused_buf_mut().redo();
                self.status_message = msg;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Save => self.handle_save(),
            EditorCommand::SaveAndQuit => {
                self.handle_save()?;
                self.handle_quit()
            }
            EditorCommand::DeleteMotion(dir, n) => {
                let buf = self.focused_buf_mut();
                buf.delete_motion(dir, n);
                buf.break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ChangeMotion(dir, n) => {
                let buf = self.focused_buf_mut();
                buf.delete_motion(dir, n);
                buf.break_undo_group();
                self.tabs[idx].notify_lsp_change();
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteLines(n) => {
                if let Some(deleted) = self.focused_buf_mut().delete_lines(n) {
                    self.yank_to_register(&deleted);
                    self.focused_buf_mut().break_undo_group();
                    self.tabs[idx].notify_lsp_change();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ChangeLines(n) => {
                let buf = self.focused_buf_mut();
                buf.delete_lines(n);
                buf.break_undo_group();
                self.tabs[idx].notify_lsp_change();
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteTextObject(modifier, kind) | EditorCommand::ChangeTextObject(modifier, kind) => {
                let is_change = matches!(cmd, EditorCommand::ChangeTextObject(..));
                let range = {
                    let buf = self.focused_buf_mut();
                    use crate::input::{TextObjectModifier, TextObjectKind};
                    match (modifier, kind) {
                        (TextObjectModifier::Inner, TextObjectKind::Word) => buf.find_inner_word(),
                        (TextObjectModifier::Around, TextObjectKind::Word) => buf.find_around_word(),
                        (TextObjectModifier::Inner, TextObjectKind::Quote(q)) => buf.find_inner_quote(q),
                        (TextObjectModifier::Around, TextObjectKind::Quote(q)) => buf.find_around_quote(q),
                        (TextObjectModifier::Inner, TextObjectKind::Bracket(o, c)) => buf.find_inner_bracket(o, c),
                        (TextObjectModifier::Around, TextObjectKind::Bracket(o, c)) => buf.find_around_bracket(o, c),
                    }
                };
                if let Some((start, end)) = range {
                    if let Some(deleted) = self.focused_buf_mut().delete_text_range(start, end) {
                        self.yank_to_register(&deleted);
                        self.focused_buf_mut().break_undo_group();
                    }
                    if is_change {
                        self.mode = EditorMode::Insert;
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteCharForward => {
                self.focused_buf_mut().delete_char_forward();
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ReplaceChar(c) => {
                self.focused_buf_mut().replace_char(c);
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenLineBelow => {
                if let Some(pos) = self.focused_buf_mut().open_line_below() {
                    self.focused_buf_mut().set_cursor_pos(pos);
                    self.focused_buf_mut().break_undo_group();
                    self.tabs[idx].notify_lsp_change();
                }
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenLineAbove => {
                if let Some(pos) = self.focused_buf_mut().open_line_above() {
                    self.focused_buf_mut().set_cursor_pos(pos);
                    self.focused_buf_mut().break_undo_group();
                    self.tabs[idx].notify_lsp_change();
                }
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::AppendEndOfLine => {
                self.focused_buf_mut().move_cursor(novim_types::Direction::LineEnd);
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::InsertStartOfLine => {
                self.focused_buf_mut().move_cursor(novim_types::Direction::LineStart);
                // Skip leading whitespace
                let cursor = self.focused_buf().cursor();
                if let Some(line) = self.focused_buf().get_line(cursor.line) {
                    let first_non_blank = line.chars().take_while(|c| c.is_whitespace()).count();
                    self.focused_buf_mut().set_cursor_pos(
                        novim_types::Position::new(cursor.line, first_non_blank),
                    );
                }
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ChangeToEnd => {
                // Delete from cursor to end of line, enter insert
                self.focused_buf_mut().delete_motion(novim_types::Direction::LineEnd, 1);
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DeleteToEnd => {
                // Delete from cursor to end of line
                if let Some(deleted) = {
                    let buf = self.focused_buf_mut();
                    buf.delete_motion(novim_types::Direction::LineEnd, 1);
                    buf.break_undo_group();
                    None::<String> // delete_motion doesn't return text, just track
                } {
                    self.yank_to_register(&deleted);
                }
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SubstituteLine => {
                // Delete line content (keep newline), enter insert
                let buf = self.focused_buf_mut();
                buf.delete_lines(1);
                buf.break_undo_group();
                self.tabs[idx].notify_lsp_change();
                self.mode = EditorMode::Insert;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::JoinLines(n) => {
                self.focused_buf_mut().join_lines(n);
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ToggleCase => {
                self.focused_buf_mut().toggle_case_at_cursor();
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Indent(n) => {
                let tw = self.config.editor.tab_width;
                let et = self.config.editor.expand_tab;
                self.focused_buf_mut().indent_lines(n, tw, et);
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Dedent(n) => {
                let tw = self.config.editor.tab_width;
                self.focused_buf_mut().dedent_lines(n, tw);
                self.focused_buf_mut().break_undo_group();
                self.tabs[idx].notify_lsp_change();
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_pane(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::SplitPane(dir) => {
                if self.focused_buf().is_terminal() {
                    self.handle_split_terminal(dir, screen_area)
                } else {
                    self.handle_split(dir)
                }
            }
            EditorCommand::FocusDirection(dir) => {
                self.tabs[idx].panes.focus_direction(dir, screen_area);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FocusNext => {
                self.tabs[idx].panes.focus_next();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ClosePane => self.handle_close_pane(),
            EditorCommand::OpenTerminal => self.handle_open_terminal(screen_area),
            EditorCommand::ForwardToTerminal(key) => {
                self.focused_buf_mut().send_key(key);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ResizePaneGrow => {
                self.tabs[idx].panes.resize_focused(0.05, true);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ResizePaneShrink => {
                self.tabs[idx].panes.resize_focused(-0.05, true);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ResizePaneWider => {
                self.tabs[idx].panes.resize_focused(0.05, false);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ResizePaneNarrower => {
                self.tabs[idx].panes.resize_focused(-0.05, false);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ZoomPane => {
                self.tabs[idx].panes.zoom_focused();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SwapPane => {
                self.tabs[idx].panes.swap_focused_with_next();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::EnterCopyMode => {
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    if pane.content.as_buffer_like().is_terminal() {
                        pane.copy_mode_offset = 1; // enter copy mode at offset 1
                        self.status_message = Some("Copy mode (q/Esc to exit, j/k to scroll)".to_string());
                    } else {
                        self.status_message = Some("Copy mode only works in terminal panes".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ExitCopyMode => {
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    pane.copy_mode_offset = 0;
                }
                self.status_message = None;
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_command_mode(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::CommandInput(c) => {
                self.command_buffer.push(c);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CommandBackspace => {
                if self.command_buffer.pop().is_none() {
                    self.mode = EditorMode::Normal;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CommandExecute => {
                let cmd_str = self.command_buffer.clone();
                if !cmd_str.is_empty() {
                    self.command_history.push(cmd_str.clone());
                }
                self.command_history_idx = self.command_history.len();
                self.mode = EditorMode::Normal;
                self.command_buffer.clear();
                let parsed = parse_ex_command(&cmd_str);
                self.execute(parsed, screen_area)
            }
            EditorCommand::CommandHistoryUp => {
                if !self.command_history.is_empty() && self.command_history_idx > 0 {
                    self.command_history_idx -= 1;
                    self.command_buffer = self.command_history[self.command_history_idx].clone();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CommandHistoryDown => {
                if self.command_history_idx + 1 < self.command_history.len() {
                    self.command_history_idx += 1;
                    self.command_buffer = self.command_history[self.command_history_idx].clone();
                } else {
                    self.command_history_idx = self.command_history.len();
                    self.command_buffer.clear();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CommandCancel => {
                self.command_buffer.clear();
                self.command_history_idx = self.command_history.len();
                self.mode = EditorMode::Normal;
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_finder(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::OpenFileFinder => {
                let root = self.tabs[idx].shell_cwd();
                self.finder.root = root.clone();
                self.finder.query.clear();
                self.finder.results = finder::find_files(&root, "", 50);
                self.finder.selected = 0;
                self.finder.visible = true;
                self.load_finder_preview();
                self.status_message = Some(format!("Find in: {}", root.display()));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenFinderAt(path) => {
                let root = {
                    let p = PathBuf::from(&path);
                    if p.is_absolute() { p } else {
                        self.tabs[idx].shell_cwd().join(p)
                    }
                };
                self.finder.root = root.clone();
                self.finder.query.clear();
                self.finder.results = finder::find_files(&root, "", 50);
                self.finder.selected = 0;
                self.finder.visible = true;
                self.status_message = Some(format!("Find in: {}", root.display()));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderInput(c) => {
                self.finder.query.push(c);
                self.finder.results = finder::find_files(&self.finder.root, &self.finder.query, 20);
                self.finder.selected = 0;
                self.load_finder_preview();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderBackspace => {
                self.finder.query.pop();
                self.finder.results = finder::find_files(&self.finder.root, &self.finder.query, 20);
                self.finder.selected = 0;
                self.load_finder_preview();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderUp => {
                if self.finder.selected > 0 {
                    self.finder.selected -= 1;
                    self.load_finder_preview();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderDown => {
                if self.finder.selected + 1 < self.finder.results.len() {
                    self.finder.selected += 1;
                    self.load_finder_preview();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderAccept => {
                if let Some(result) = self.finder.results.get(self.finder.selected) {
                    let path = result.path.to_string_lossy().to_string();
                    self.finder.visible = false;
                    self.handle_edit_file(&path)?;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FinderDismiss => {
                self.finder.visible = false;
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_workspace(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::OpenTab(path) => {
                let dir = resolve_path(&path, &self.tabs[idx].panes, self.tabs[idx].last_shell_cwd.as_ref());
                let name = dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "new".to_string());
                let ws = Workspace::new_terminal_at(&name, &dir, Arc::clone(&self.lsp_registry), screen_area);
                self.tabs.push(ws);
                self.active_tab = self.tabs.len() - 1;
                self.status_message = Some(format!("Workspace: {}", name));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::NextTab => {
                if self.tabs.len() > 1 {
                    self.active_tab = (self.active_tab + 1) % self.tabs.len();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PrevTab => {
                if self.tabs.len() > 1 {
                    self.active_tab = if self.active_tab == 0 { self.tabs.len() - 1 } else { self.active_tab - 1 };
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CloseTab => {
                if self.tabs.len() > 1 {
                    self.tabs.remove(self.active_tab);
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len() - 1;
                    }
                } else {
                    self.status_message = Some("Cannot close last workspace".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::JumpToTab(n) => {
                if n < self.tabs.len() {
                    self.active_tab = n;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ListWorkspaces => {
                self.show_workspace_list = !self.show_workspace_list;
                self.workspace_list_selected = self.active_tab;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::RenameTab(name) => {
                self.tabs[self.active_tab].name = name.clone();
                self.status_message = Some(format!("Workspace renamed to: {}", name));
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_lsp_commands(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::ShowHover => {
                self.with_lsp_client(|client, uri, cursor| {
                    let _ = client.hover(uri, cursor.line as u32, cursor.column as u32);
                    None
                });
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::GotoDefinition => {
                self.push_jump();
                let msg = self.with_lsp_client(|client, uri, cursor| {
                    match client.goto_definition(uri, cursor.line as u32, cursor.column as u32) {
                        Ok(()) => Some("Looking up definition...".to_string()),
                        Err(_) => Some("No LSP server running".to_string()),
                    }
                });
                if let Some(msg) = msg {
                    self.status_message = Some(msg);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::TriggerCompletion => {
                let msg = self.with_lsp_client(|client, uri, cursor| {
                    match client.completion(uri, cursor.line as u32, cursor.column as u32) {
                        Ok(()) => Some("Requesting completions...".to_string()),
                        Err(e) => Some(format!("Completion error: {}", e)),
                    }
                });
                self.status_message = msg.or(Some("No LSP available".to_string()));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CompletionUp => {
                if self.completion.visible && self.completion.selected > 0 {
                    self.completion.selected -= 1;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CompletionDown => {
                if self.completion.visible && self.completion.selected + 1 < self.completion.items.len() {
                    self.completion.selected += 1;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CompletionAccept => {
                if self.completion.visible {
                    if let Some(item) = self.completion.items.get(self.completion.selected) {
                        let text = item.insert_text.clone().unwrap_or_else(|| item.label.clone());
                        let buf = self.focused_buf_mut();
                        for c in text.chars() {
                            buf.insert_char(c);
                        }
                        self.tabs[idx].notify_lsp_change();
                    }
                    self.completion.visible = false;
                    self.completion.items.clear();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CompletionDismiss => {
                self.completion.visible = false;
                self.completion.items.clear();
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_explorer(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::ToggleExplorer => {
                if self.tabs[idx].explorer.is_some() {
                    self.tabs[idx].explorer = None;
                    self.tabs[idx].explorer_focused = false;
                } else {
                    self.open_explorer_at(None);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenExplorer(path) => {
                self.open_explorer_at(Some(&path));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FocusExplorer => {
                if self.tabs[idx].explorer.is_some() {
                    self.tabs[idx].explorer_focused = true;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ExplorerUp => {
                if let Some(exp) = &mut self.tabs[idx].explorer {
                    exp.cursor_up();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ExplorerDown => {
                if let Some(exp) = &mut self.tabs[idx].explorer {
                    exp.cursor_down();
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ExplorerOpen => {
                let file_to_open = self.tabs[idx].explorer.as_mut().and_then(|exp| exp.open_at_cursor());
                if let Some(path) = file_to_open {
                    let path_str = path.to_string_lossy().to_string();
                    self.handle_edit_file(&path_str)?;
                    self.tabs[self.active_tab].explorer_focused = false;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ToggleHelp => {
                self.show_help = !self.show_help;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::DismissPopup => {
                self.show_help = false;
                self.tabs[self.active_tab].show_buffer_list = false;
                self.show_workspace_list = false;
                self.hover_text = None;
                self.plugin_popup = None;
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_search(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::EnterSearch => {
                self.search.active = true;
                self.search.buffer.clear();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchInput(c) => {
                self.search.buffer.push(c);
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchBackspace => {
                if self.search.buffer.pop().is_none() {
                    self.search.active = false;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchExecute => {
                self.push_jump();
                let pattern = self.search.buffer.clone();
                self.search.active = false;
                if !pattern.is_empty() {
                    self.search.pattern = Some(pattern.clone());
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_forward(&pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                        self.status_message = Some(format!("/{}", pattern));
                    } else {
                        self.status_message = Some(format!("Pattern not found: {}", pattern));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchCancel => {
                self.search.active = false;
                self.search.buffer.clear();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::NextMatch => {
                if let Some(pattern) = self.search.pattern.clone() {
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_forward(&pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    } else {
                        self.status_message = Some("No more matches".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PrevMatch => {
                if let Some(pattern) = self.search.pattern.clone() {
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_backward(&pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    } else {
                        self.status_message = Some("No more matches".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ClearSearch => {
                self.search.pattern = None;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ReplaceAll(pattern, replacement, case_insensitive) => {
                let effective_pattern = if case_insensitive {
                    format!("(?i){}", pattern)
                } else {
                    pattern
                };
                let count = self.focused_buf_mut().replace_all(&effective_pattern, &replacement);
                self.status_message = Some(format!("{} replacements made", count));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchWordForward => {
                if let Some(word) = self.focused_buf().word_at_cursor() {
                    let pattern = format!(r"\b{}\b", regex::escape(&word));
                    self.search.pattern = Some(pattern.clone());
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_forward(&pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    }
                    self.status_message = Some(format!("/{}", pattern));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SearchWordBackward => {
                if let Some(word) = self.focused_buf().word_at_cursor() {
                    let pattern = format!(r"\b{}\b", regex::escape(&word));
                    self.search.pattern = Some(pattern.clone());
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_backward(&pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    }
                    self.status_message = Some(format!("?{}", pattern));
                }
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_scroll_buffer(&mut self, cmd: EditorCommand, screen_area: novim_types::Rect) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::ScrollCenter => {
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let cursor_line = pane.content.as_buffer_like().cursor().line;
                    let half = (screen_area.height as usize) / 2;
                    pane.viewport_offset = cursor_line.saturating_sub(half);
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ScrollTop => {
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let cursor_line = pane.content.as_buffer_like().cursor().line;
                    pane.viewport_offset = cursor_line;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ScrollBottom => {
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let cursor_line = pane.content.as_buffer_like().cursor().line;
                    let height = screen_area.height as usize;
                    pane.viewport_offset = cursor_line.saturating_sub(height.saturating_sub(1));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ScrollUp => {
                let n = self.config.editor.scroll_lines;
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    pane.viewport_offset = pane.viewport_offset.saturating_sub(n);
                    let cursor = pane.content.as_buffer_like().cursor();
                    if cursor.line > pane.viewport_offset + n {
                        pane.content.as_buffer_like_mut().set_cursor_pos(
                            novim_types::Position::new(pane.viewport_offset, cursor.column),
                        );
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ScrollDown => {
                let n = self.config.editor.scroll_lines;
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let max = pane.content.as_buffer_like().len_lines().saturating_sub(1);
                    pane.viewport_offset = (pane.viewport_offset + n).min(max);
                    let cursor = pane.content.as_buffer_like().cursor();
                    if cursor.line < pane.viewport_offset {
                        pane.content.as_buffer_like_mut().set_cursor_pos(
                            novim_types::Position::new(pane.viewport_offset, cursor.column),
                        );
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PageUp => {
                let page_height = screen_area.height.saturating_sub(2) as usize; // leave status/tab lines
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    pane.viewport_offset = pane.viewport_offset.saturating_sub(page_height);
                    let cursor = pane.content.as_buffer_like().cursor();
                    let new_line = cursor.line.saturating_sub(page_height);
                    pane.content.as_buffer_like_mut().set_cursor_pos(
                        novim_types::Position::new(new_line, cursor.column),
                    );
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PageDown => {
                let page_height = screen_area.height.saturating_sub(2) as usize;
                let focused_id = self.tabs[idx].panes.focused_id();
                if let Some(pane) = self.tabs[idx].panes.get_pane_mut(focused_id) {
                    let max = pane.content.as_buffer_like().len_lines().saturating_sub(1);
                    pane.viewport_offset = (pane.viewport_offset + page_height).min(max);
                    let cursor = pane.content.as_buffer_like().cursor();
                    let new_line = (cursor.line + page_height).min(max);
                    pane.content.as_buffer_like_mut().set_cursor_pos(
                        novim_types::Position::new(new_line, cursor.column),
                    );
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::BufferNext => {
                if self.tabs[idx].buffer_history.len() > 1 {
                    self.tabs[idx].buffer_history_idx = (self.tabs[idx].buffer_history_idx + 1) % self.tabs[idx].buffer_history.len();
                    let path = self.tabs[idx].buffer_history[self.tabs[idx].buffer_history_idx].clone();
                    self.handle_edit_file(&path)?;
                } else {
                    self.status_message = Some("No other buffers".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::BufferPrev => {
                if self.tabs[idx].buffer_history.len() > 1 {
                    self.tabs[idx].buffer_history_idx = if self.tabs[idx].buffer_history_idx == 0 {
                        self.tabs[idx].buffer_history.len() - 1
                    } else {
                        self.tabs[idx].buffer_history_idx - 1
                    };
                    let path = self.tabs[idx].buffer_history[self.tabs[idx].buffer_history_idx].clone();
                    self.handle_edit_file(&path)?;
                } else {
                    self.status_message = Some("No other buffers".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::BufferList => {
                self.tabs[idx].show_buffer_list = !self.tabs[idx].show_buffer_list;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SetOption(opt) => self.handle_set_option(&opt),
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_macros(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::StartMacroRecord(reg) => {
                if self.macros.recording.is_some() {
                    if let Some(recording_reg) = self.macros.recording.take() {
                        self.macros.registers.insert(recording_reg, std::mem::take(&mut self.macros.buffer));
                        self.status_message = Some(format!("Recorded @{}", recording_reg));
                    }
                } else {
                    self.macros.recording = Some(reg);
                    self.macros.buffer.clear();
                    self.status_message = Some(format!("Recording @{}...", reg));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::StopMacroRecord => {
                if let Some(reg) = self.macros.recording.take() {
                    self.macros.registers.insert(reg, std::mem::take(&mut self.macros.buffer));
                    self.status_message = Some(format!("Recorded @{}", reg));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ReplayMacro(reg) => {
                let actual_reg = if reg == '@' {
                    self.macros.last_register.unwrap_or('a')
                } else {
                    reg
                };
                self.macros.last_register = Some(actual_reg);

                if let Some(keys) = self.macros.registers.get(&actual_reg).cloned() {
                    self.status_message = Some(format!("Replaying @{} ({} keys)", actual_reg, keys.len()));
                    for key in keys {
                        let in_terminal = self.focused_buf().is_terminal();
                        let popup_showing = self.show_help || self.tabs[self.active_tab].show_buffer_list;
                        let (cmd, new_input_state) =
                            key_to_command(self.mode, self.input_state, key, in_terminal, popup_showing, false);
                        self.input_state = new_input_state;
                        self.execute(cmd, screen_area)?;
                    }
                } else {
                    self.status_message = Some(format!("Register @{} is empty", actual_reg));
                }
                Ok(ExecOutcome::Continue)
            }
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn execute_marks_jumps(&mut self, cmd: EditorCommand) -> Result<ExecOutcome, NovimError> {
        match cmd {
            EditorCommand::SetMark(c) => {
                let path = self.focused_buf().display_name();
                let pos = self.focused_buf().cursor();
                self.marks.insert(c, (path, pos));
                self.status_message = Some(format!("Mark '{}' set", c));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::JumpToMark(c, exact) => {
                if let Some((path, pos)) = self.marks.get(&c).cloned() {
                    self.push_jump();
                    let current_name = self.focused_buf().display_name();
                    if path != current_name {
                        let _ = self.handle_edit_file(&path);
                    }
                    if exact {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    } else {
                        self.focused_buf_mut().set_cursor_pos(novim_types::Position::new(pos.line, 0));
                    }
                } else {
                    self.status_message = Some(format!("Mark '{}' not set", c));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ListMarks => {
                let mut items: Vec<String> = self.marks.iter()
                    .map(|(k, (path, pos))| format!("'{}  {}:{}:{}", k, path, pos.line + 1, pos.column + 1))
                    .collect();
                items.sort();
                if items.is_empty() {
                    self.status_message = Some("No marks set".to_string());
                } else {
                    self.status_message = Some(items.join(" | "));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::JumpBack => self.navigate_jump_list(false),
            EditorCommand::JumpForward => self.navigate_jump_list(true),
            _ => Ok(ExecOutcome::Continue),
        }
    }

    pub(super) fn handle_quit(&mut self) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        if self.focused_buf().is_dirty() {
            return Err(NovimError::Buffer(
                "Unsaved changes! Use :q! to force quit or :wq to save and quit".to_string(),
            ));
        }
        if self.tabs[idx].panes.pane_count() > 1 {
            self.tabs[idx].panes.close_focused();
            Ok(ExecOutcome::Continue)
        } else {
            Ok(ExecOutcome::Quit)
        }
    }

    pub(super) fn handle_save(&mut self) -> Result<ExecOutcome, NovimError> {
        match self.focused_buf_mut().save() {
            Ok(msg) => self.status_message = Some(msg),
            Err(e) => return Err(e),
        }
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_split(&mut self, direction: SplitDirection) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        self.tabs[idx].panes.split(direction);
        self.status_message = Some(format!("Split ({} panes)", self.tabs[idx].panes.pane_count()));
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_split_terminal(&mut self, direction: SplitDirection, screen_area: novim_types::Rect) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        let rows = (screen_area.height / 2).max(5);
        let cols = screen_area.width.saturating_sub(2);
        match self.tabs[idx].panes.split_terminal(direction, rows, cols) {
            Ok(()) => {
                self.status_message = Some(format!("Split ({} panes)", self.tabs[idx].panes.pane_count()));
            }
            Err(e) => return Err(NovimError::Io(e)),
        }
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_close_pane(&mut self) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        if self.tabs[idx].panes.pane_count() > 1 {
            self.tabs[idx].panes.close_focused();
        } else {
            return Err(NovimError::Command("Cannot close last pane".to_string()));
        }
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_save_session(&mut self, name: &str) -> Result<ExecOutcome, NovimError> {
        let workspaces: Vec<(String, &crate::pane::PaneManager, String)> = self.tabs
            .iter()
            .map(|ws| (ws.name.clone(), &ws.panes, ws.launch_dir.to_string_lossy().to_string()))
            .collect();
        let captured = session::capture_multi_session(name, &workspaces, self.active_tab);
        match session::save_session(&captured) {
            Ok(msg) => self.status_message = Some(format!("{} ({} workspaces)", msg, self.tabs.len())),
            Err(e) => return Err(NovimError::Session(e.to_string())),
        }
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_open_terminal(&mut self, screen_area: novim_types::Rect) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        let rows = (screen_area.height / 2).max(5);
        let cols = screen_area.width.saturating_sub(2);

        // If the only pane is an empty unnamed buffer, replace it instead of splitting
        let pane = self.tabs[idx].panes.focused_pane();
        let buf = pane.content.as_buffer_like();
        let is_terminal = buf.is_terminal();
        let display_name = buf.display_name();
        let line_count = buf.len_lines();
        let pane_count = self.tabs[idx].panes.pane_count();
        let is_empty_editor = !is_terminal
            && display_name == "[No Name]"
            && line_count <= 1
            && pane_count == 1;

        if is_empty_editor {
            match crate::emulator::TerminalPane::new(rows, cols) {
                Ok(term) => {
                    let pane = self.tabs[idx].panes.focused_pane_mut();
                    pane.content = PaneContent::Terminal(term);
                    self.status_message = Some("[Terminal] opened".to_string());
                }
                Err(e) => return Err(NovimError::Io(e)),
            }
        } else {
            match self.tabs[idx].panes.split_terminal(SplitDirection::Horizontal, rows, cols) {
                Ok(()) => self.status_message = Some("[Terminal] opened".to_string()),
                Err(e) => return Err(NovimError::Io(e)),
            }
        }
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn handle_set_option(&mut self, opt: &str) -> Result<ExecOutcome, NovimError> {
        // Query: `:set all` shows all options, `:set tabstop?` shows one
        if opt == "all" {
            self.status_message = Some(format!(
                "ts={} et={} ai={} wrap={} ln={}",
                self.config.editor.tab_width,
                self.config.editor.expand_tab,
                self.config.editor.auto_indent,
                self.config.editor.word_wrap,
                self.config.editor.line_numbers,
            ));
            return Ok(ExecOutcome::Continue);
        }
        if let Some(name) = opt.strip_suffix('?') {
            let val = match name {
                "tabstop" | "ts" => format!("tabstop={}", self.config.editor.tab_width),
                "expandtab" | "et" => format!("expandtab={}", self.config.editor.expand_tab),
                "autoindent" | "ai" => format!("autoindent={}", self.config.editor.auto_indent),
                "wrap" => format!("wrap={}", self.config.editor.word_wrap),
                "number" | "nu" | "rnu" | "nonu" => format!("line_numbers={}", self.config.editor.line_numbers),
                _ => format!("Unknown option: {}", name),
            };
            self.status_message = Some(val);
            return Ok(ExecOutcome::Continue);
        }
        match opt {
            "number" | "nu" => {
                self.line_number_mode = LineNumberMode::Absolute;
                self.status_message = Some("Line numbers: absolute".to_string());
            }
            "relativenumber" | "rnu" => {
                self.line_number_mode = LineNumberMode::Hybrid;
                self.status_message = Some("Line numbers: hybrid (relative)".to_string());
            }
            "norelativenumber" | "nornu" => {
                self.line_number_mode = LineNumberMode::Absolute;
                self.status_message = Some("Line numbers: absolute".to_string());
            }
            "nonumber" | "nonu" => {
                self.line_number_mode = LineNumberMode::Off;
                self.status_message = Some("Line numbers: off".to_string());
            }
            "expandtab" | "et" => {
                self.config.editor.expand_tab = true;
                self.status_message = Some("expandtab on".to_string());
            }
            "noexpandtab" | "noet" => {
                self.config.editor.expand_tab = false;
                self.status_message = Some("expandtab off".to_string());
            }
            "autoindent" | "ai" => {
                self.config.editor.auto_indent = true;
                self.status_message = Some("autoindent on".to_string());
            }
            "noautoindent" | "noai" => {
                self.config.editor.auto_indent = false;
                self.status_message = Some("autoindent off".to_string());
            }
            "wrap" => {
                self.config.editor.word_wrap = true;
                self.status_message = Some("wrap on".to_string());
            }
            "nowrap" => {
                self.config.editor.word_wrap = false;
                self.status_message = Some("wrap off".to_string());
            }
            _ if opt.starts_with("tabstop=") || opt.starts_with("ts=") => {
                let val = opt.split('=').nth(1).unwrap_or("4");
                if let Ok(tw) = val.parse::<usize>() {
                    self.config.editor.tab_width = tw.clamp(1, 16);
                    self.status_message = Some(format!("tabstop={}", self.config.editor.tab_width));
                } else {
                    return Err(NovimError::Command(format!("Invalid tabstop: {}", val)));
                }
            }
            _ => {
                return Err(NovimError::Command(format!("Unknown option: {}", opt)));
            }
        }
        Ok(ExecOutcome::Continue)
    }

    pub fn handle_edit_file(&mut self, path: &str) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        let buffer = Buffer::from_file(path)?;
        // Always replace the focused pane. If a terminal is destroyed,
        // last_shell_cwd keeps its CWD cached for explorer/finder.
        let pane = self.tabs[idx].panes.focused_pane_mut();
        pane.content = PaneContent::Editor(buffer);
        pane.viewport_offset = 0;
        self.status_message = Some(format!("Editing: {}", path));
        let path_str = path.to_string();
        if !self.tabs[idx].buffer_history.contains(&path_str) {
            self.tabs[idx].buffer_history.push(path_str.clone());
        }
        self.tabs[idx].buffer_history_idx = self.tabs[idx].buffer_history
            .iter()
            .position(|p| p == &path_str)
            .unwrap_or(0);
        if self.config.lsp.enabled {
            self.tabs[idx].ensure_lsp_for_buffer(self.config.lsp.enabled);
        }
        let tw = self.config.editor.tab_width;
        self.focused_buf_mut().recompute_folds(tw);
        Ok(ExecOutcome::Continue)
    }

    pub(super) fn open_explorer_at(&mut self, path: Option<&str>) {
        let idx = self.active_tab;
        let dir = match path {
            Some(".") | None => self.tabs[idx].shell_cwd(),
            Some(p) => {
                let p = PathBuf::from(p);
                if p.is_absolute() { p } else { self.tabs[idx].shell_cwd().join(p) }
            }
        };
        match Explorer::new(&dir) {
            Ok(exp) => {
                self.tabs[idx].explorer = Some(exp);
                self.tabs[idx].explorer_focused = true;
            }
            Err(e) => self.status_message = Some(format!("Explorer: {}", e)),
        }
    }

    /// Load preview content for the currently selected finder result.
    pub fn load_finder_preview(&mut self) {
        self.finder.preview_lines.clear();
        self.finder.preview_highlights.clear();
        if !self.config.editor.finder_preview {
            return;
        }
        if let Some(result) = self.finder.results.get(self.finder.selected) {
            if let Ok(content) = std::fs::read_to_string(&result.path) {
                let preview: String = content.lines().take(200).collect::<Vec<_>>().join("\n");
                self.finder.preview_lines = preview.lines().map(|l| l.to_string()).collect();

                let ext = result.path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if let Some(hl) = highlight::SyntaxHighlighter::from_extension(ext) {
                    self.finder.preview_highlights = hl.highlight(&preview);
                }
            } else {
                self.finder.preview_lines = vec!["(binary or unreadable file)".to_string()];
            }
        }
    }
}
