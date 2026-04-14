//! Editor state machine — shared between TUI and GUI frontends.
//!
//! Contains: EditorState, Workspace, sub-states, command execution.
//! This module has NO dependency on Ratatui or any rendering crate.
//! Input events use crossterm types as the common format.

mod types;
mod workspace;
mod handlers;
mod input;
mod completion;

pub use types::*;
pub use workspace::*;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::buffer::BufferLike;
use crate::config::{self, NovimConfig};
use crate::error::NovimError;
use crate::input::{CountState, EditorCommand, InputState};
use crate::lsp::provider::LspRegistry;
use crate::pane::PaneManager;
use crate::plugin::manager::PluginManager;
use crate::session;
use novim_types::EditorMode;

/// Editor state — owns workspace tabs, mode, and transient UI state.
pub struct EditorState {
    // Workspace tabs
    pub tabs: Vec<Workspace>,
    pub active_tab: usize,
    // Shared state (NOT per-workspace)
    pub mode: EditorMode,
    pub input_state: InputState,
    pub count_state: CountState,
    pub status_message: Option<String>,
    pub command_buffer: String,
    pub config: NovimConfig,
    pub lsp_registry: Arc<LspRegistry>,
    /// Named registers (a-z, ", +). Unnamed register is '"'.
    pub registers: HashMap<char, String>,
    /// Pending register for the next yank/delete/paste operation.
    pub pending_register: Option<char>,
    pub show_help: bool,
    pub help_scroll: usize,
    pub line_number_mode: LineNumberMode,
    // Sub-states
    pub search: SearchState,
    pub finder: FinderState,
    pub completion: CompletionState,
    pub macros: MacroState,
    pub hover_text: Option<String>,
    // Workspace list popup
    pub show_workspace_list: bool,
    pub workspace_list_selected: usize,
    /// Show the welcome/splash screen (dismissed on first keypress).
    pub show_welcome: bool,
    /// Plugin popup overlay (title, lines, scroll, width, height).
    pub plugin_popup: Option<PluginPopup>,
    /// Jump list for Ctrl+O / Ctrl+I navigation.
    pub jump_list: Vec<(String, novim_types::Position)>,
    pub jump_index: usize,
    /// Named marks (a-z) → (file_path, position).
    pub marks: HashMap<char, (String, novim_types::Position)>,
    /// Current git branch name (for status bar).
    pub git_branch: Option<String>,
    /// Last edit for dot repeat.
    pub last_edit: Option<EditRecord>,
    /// Whether we're recording insert-mode text for dot repeat.
    pub(crate) recording_insert: bool,
    /// Accumulated insert-mode keystrokes for dot repeat.
    pub(crate) insert_record: Vec<EditorCommand>,
    /// Plugin system manager.
    pub plugins: PluginManager,
    /// Last visual selection for gv (reselect).
    pub last_visual_selection: Option<novim_types::Selection>,
    /// Last f/F/t/T find for ; and , repeat.
    pub last_find_char: Option<(char, bool, bool)>,
    /// Command-line history for up/down recall.
    pub command_history: Vec<String>,
    pub command_history_idx: usize,
    /// Interactive confirm-replace state (`:s///c`).
    pub confirm_replace: ConfirmReplaceState,
    /// Tab completion state for command mode.
    pub cmd_completion: CommandCompletionState,
    /// Quickfix list state.
    pub quickfix: QuickfixState,
    /// Command history window state.
    pub command_window: CommandWindowState,
    /// Symbol list popup state.
    pub symbol_list: SymbolListState,
    /// Floating windows (plugin-created).
    pub floating_windows: Vec<FloatingWindow>,
    /// Symbol outline sidebar state.
    pub outline: OutlineState,
    /// Background task runner for non-blocking operations.
    pub tasks: crate::async_task::TaskRunner,
}

impl EditorState {
    /// Shorthand: reference to the active workspace.
    pub fn ws(&self) -> &Workspace {
        &self.tabs[self.active_tab]
    }

    /// Shorthand: mutable reference to the active workspace.
    pub fn ws_mut(&mut self) -> &mut Workspace {
        &mut self.tabs[self.active_tab]
    }

    /// Shorthand: immutable reference to the focused pane's buffer-like content.
    pub fn focused_buf(&self) -> &dyn BufferLike {
        self.tabs[self.active_tab].panes.focused_pane().content.as_buffer_like()
    }

    /// Shorthand: mutable reference to the focused pane's buffer-like content.
    pub fn focused_buf_mut(&mut self) -> &mut dyn BufferLike {
        self.tabs[self.active_tab].panes.focused_pane_mut().content.as_buffer_like_mut()
    }

    /// Compute status bar content for renderers. Avoids duplicating this logic
    /// across TUI and GUI frontends.
    pub fn status_bar_info(&self) -> StatusBarInfo {
        let idx = self.active_tab;
        let ws = &self.tabs[idx];
        let pane = ws.panes.focused_pane();
        let buf = pane.content.as_buffer_like();
        let cursor = buf.cursor();
        let total = buf.len_lines();
        let pane_count = ws.panes.pane_count();

        let mode_name = if let Some(reg) = self.macros.recording {
            format!("REC @{}", reg)
        } else if self.input_state == crate::input::InputState::WaitingPaneCommand {
            "CTRL+W...".to_string()
        } else if buf.is_terminal() {
            "TERMINAL".to_string()
        } else {
            self.mode.display_name().to_string()
        };

        let diag_summary = {
            let uri = match &pane.content {
                crate::pane::PaneContent::Editor(b) => b.file_uri(),
                _ => None,
            };
            if let Some(diags) = uri.and_then(|u| ws.diagnostics.get(&u)) {
                let errors = diags.iter().filter(|d| d.severity == crate::lsp::DiagnosticSeverity::Error).count();
                let warnings = diags.iter().filter(|d| d.severity == crate::lsp::DiagnosticSeverity::Warning).count();
                if errors > 0 || warnings > 0 {
                    format!(" {}E {}W", errors, warnings)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };

        let lsp_status = ws.lsp_status.as_ref()
            .map(|s| format!(" {}", s))
            .unwrap_or_default();

        let pane_info = if pane_count > 1 {
            format!(" [pane {}/{}]", ws.panes.focused_id() + 1, pane_count)
        } else {
            String::new()
        };

        let git_branch = self.git_branch.as_deref().map(|b| format!(" {}", b)).unwrap_or_default();

        StatusBarInfo {
            mode_name,
            diag_summary,
            lsp_status,
            pane_info,
            git_branch,
            cursor_line: cursor.line + 1,
            cursor_col: cursor.column + 1,
            total_lines: total,
            status_message: self.status_message.clone(),
            file_name: buf.display_name(),
            is_dirty: buf.is_dirty(),
        }
    }

    fn with_config_and_tabs(cfg: NovimConfig, registry: Arc<LspRegistry>, tabs: Vec<Workspace>, active_tab: usize, status_message: Option<String>) -> Self {
        let ln_mode = ln_mode_from_config(&cfg.editor.line_numbers);
        let mut plugins = PluginManager::new(false, std::collections::HashMap::new());
        crate::plugin::builtins::register_builtins(&mut plugins);
        crate::plugin::builtins::register_lsp(&mut plugins, Arc::clone(&registry));
        plugins.load_lua_plugins();
        // Git branch detection runs async to avoid blocking startup
        let git_branch = None;
        let load_errors = plugins.take_load_errors();
        let status_message = if !load_errors.is_empty() {
            Some(format!("Plugin errors: {}", load_errors.join("; ")))
        } else {
            status_message
        };
        let mut state = Self {
            tabs,
            active_tab,
            mode: EditorMode::default(),
            input_state: InputState::Ready,
            status_message,
            command_buffer: String::new(),
            show_help: false,
            help_scroll: 0,
            line_number_mode: ln_mode,
            registers: HashMap::new(),
            pending_register: None,
            config: cfg,
            lsp_registry: registry,
            count_state: CountState::default(),
            search: SearchState::default(),
            finder: FinderState::default(),
            completion: CompletionState::default(),
            macros: MacroState::default(),
            hover_text: None,
            show_workspace_list: false,
            workspace_list_selected: 0,
            show_welcome: false,
            plugin_popup: None,
            jump_list: Vec::new(),
            jump_index: 0,
            marks: HashMap::new(),
            git_branch,
            last_edit: None,
            recording_insert: false,
            insert_record: Vec::new(),
            plugins,
            last_visual_selection: None,
            last_find_char: None,
            command_history: Vec::new(),
            command_history_idx: 0,
            confirm_replace: ConfirmReplaceState::default(),
            cmd_completion: CommandCompletionState::default(),
            quickfix: QuickfixState::default(),
            command_window: CommandWindowState::default(),
            symbol_list: SymbolListState::default(),
            floating_windows: Vec::new(),
            outline: OutlineState::default(),
            tasks: crate::async_task::TaskRunner::new(),
        };

        // Detect git branch in background (non-blocking startup)
        state.tasks.spawn(|| {
            let branch = std::process::Command::new("git")
                .args(["branch", "--show-current"])
                .output()
                .ok()
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() { None } else { Some(s) }
                });
            vec![crate::async_task::TaskResult::GitBranch(branch)]
        });

        // Fire BufOpen for the initial buffer so the LSP plugin starts
        let snapshot = state.make_buffer_snapshot();
        if snapshot.path.is_some() {
            let path = snapshot.path.clone().unwrap();
            let screen = novim_types::Rect::new(0, 0, 80, 24);
            let event = crate::plugin::EditorEvent::BufOpen { path };
            let actions = state.plugins.dispatch(&event, &snapshot);
            state.run_plugin_actions(actions, screen);
        }

        state
    }

    pub fn new_editor() -> Self {
        let cfg = config::load_config();
        let registry = Arc::new(LspRegistry::from_config(&cfg));
        let ws = Workspace::new_editor("novim", Arc::clone(&registry));
        Self::with_config_and_tabs(cfg, registry, vec![ws], 0, None)
    }

    /// Create a new editor state with the welcome screen shown (no terminal pane).
    pub fn new_welcome() -> Self {
        let mut state = Self::new_editor();
        state.show_welcome = true;
        state
    }

    pub fn with_file(path: &str) -> io::Result<Self> {
        let cfg = config::load_config();
        let registry = Arc::new(LspRegistry::from_config(&cfg));
        let ws = Workspace::with_file("novim", path, Arc::clone(&registry))?;
        Ok(Self::with_config_and_tabs(cfg, registry, vec![ws], 0, None))
    }

    /// Open a directory: empty editor pane + explorer sidebar.
    pub fn with_dir(path: &str) -> io::Result<Self> {
        let mut state = Self::new_editor();
        state.open_explorer_at(Some(path));
        state.status_message = Some("Select a file from the explorer".to_string());
        Ok(state)
    }

    pub fn new_terminal(rows: u16, cols: u16) -> io::Result<Self> {
        let cfg = config::load_config();
        let registry = Arc::new(LspRegistry::from_config(&cfg));
        let ws = Workspace::new_terminal("terminal", rows, cols, Arc::clone(&registry))?;
        Ok(Self::with_config_and_tabs(cfg, registry, vec![ws], 0, None))
    }

    pub fn from_session(name: &str) -> io::Result<Self> {
        let cfg = config::load_config();
        let registry = Arc::new(LspRegistry::from_config(&cfg));
        let sess = session::load_session(name)
            .map_err(io::Error::other)?;

        let (ws_data, active) = session::restore_multi_session(&sess)
            .map_err(io::Error::other)?;

        let tabs: Vec<Workspace> = ws_data
            .into_iter()
            .map(|(ws_name, panes, dir)| {
                let mut ws = Workspace::from_session(&ws_name, panes, Arc::clone(&registry));
                ws.launch_dir = std::path::PathBuf::from(dir);
                ws
            })
            .collect();

        let tab_count = tabs.len();
        Ok(Self::with_config_and_tabs(
            cfg,
            registry,
            tabs,
            active,
            Some(format!("Session '{}' restored ({} workspaces)", name, tab_count)),
        ))
    }

    /// Execute a command. Returns Ok(Quit) to exit, Ok(Continue) to keep going,
    /// or Err with structured error information.
    pub fn execute(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        // Clear previous status message on new command (unless it's Noop)
        if !matches!(cmd, EditorCommand::Noop) {
            self.status_message = None;
        }

        // ── Dot repeat tracking ──
        // If recording insert mode text, capture insert commands
        if self.recording_insert {
            match &cmd {
                EditorCommand::InsertChar(_) | EditorCommand::InsertTab
                | EditorCommand::InsertNewline | EditorCommand::DeleteCharBefore => {
                    self.insert_record.push(cmd.clone());
                }
                EditorCommand::SwitchMode(EditorMode::Normal) => {
                    // Finalize: save the edit record with insert text
                    self.recording_insert = false;
                    if let Some(edit) = &mut self.last_edit {
                        edit.insert_text = std::mem::take(&mut self.insert_record);
                    }
                }
                _ => {}
            }
        }
        // Detect edit commands to record for dot repeat (skip DotRepeat itself)
        if !matches!(cmd, EditorCommand::DotRepeat | EditorCommand::Noop
            | EditorCommand::SwitchMode(_) | EditorCommand::MoveCursor(_)
            | EditorCommand::MoveCursorN(..) | EditorCommand::SelectRegister(_)) {
            let is_edit = matches!(cmd,
                EditorCommand::DeleteLines(_) | EditorCommand::DeleteMotion(..)
                | EditorCommand::ChangeMotion(..) | EditorCommand::ChangeLines(_)
                | EditorCommand::DeleteSelection | EditorCommand::Paste | EditorCommand::PasteBefore
                | EditorCommand::DeleteTextObject(..) | EditorCommand::ChangeTextObject(..)
                | EditorCommand::YankLines(_) | EditorCommand::InsertChar(_)
            );
            if is_edit && !self.recording_insert {
                self.last_edit = Some(EditRecord {
                    command: cmd.clone(),
                    insert_text: Vec::new(),
                });
                // If this is a Change command, start recording insert text
                let enters_insert = matches!(cmd,
                    EditorCommand::ChangeMotion(..) | EditorCommand::ChangeLines(_)
                    | EditorCommand::ChangeTextObject(..)
                );
                if enters_insert {
                    self.recording_insert = true;
                    self.insert_record.clear();
                }
            }
        }

        let old_mode = self.mode;
        let events = Self::events_for_command(&cmd, self);
        let result = self.execute_inner(cmd, screen_area);
        if result.is_ok() {
            let snapshot = self.make_buffer_snapshot();
            // Emit mode change if it changed
            if self.mode != old_mode {
                let actions = self.plugins.dispatch(
                    &crate::plugin::EditorEvent::ModeChanged {
                        from: old_mode.display_name().to_string(),
                        to: self.mode.display_name().to_string(),
                    },
                    &snapshot,
                );
                self.run_plugin_actions(actions, screen_area);
            }
            // Emit command-specific events
            for event in events {
                let actions = self.plugins.dispatch(&event, &snapshot);
                self.run_plugin_actions(actions, screen_area);
            }
            // Update breadcrumb if outline is visible
            if self.outline.visible {
                self.update_breadcrumb();
            }
        }
        result
    }

    fn execute_inner(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        match &cmd {
            // Navigation
            EditorCommand::MoveCursor(..)
            | EditorCommand::MoveCursorN(..)
            | EditorCommand::AddCursorAbove
            | EditorCommand::AddCursorBelow
            | EditorCommand::ClearSecondaryCursors
            | EditorCommand::FindChar(..)
            | EditorCommand::TillChar(..)
            | EditorCommand::RepeatFindChar
            | EditorCommand::RepeatFindCharReverse
            | EditorCommand::MatchBracket
            | EditorCommand::DisplayLineDown
            | EditorCommand::DisplayLineUp => self.execute_navigation(cmd),

            // Folds
            EditorCommand::ToggleFold
            | EditorCommand::FoldAll
            | EditorCommand::UnfoldAll => self.execute_fold(cmd),

            // Visual mode
            EditorCommand::EnterVisual
            | EditorCommand::EnterVisualBlock
            | EditorCommand::DeleteSelection
            | EditorCommand::YankSelection
            | EditorCommand::VisualIndent
            | EditorCommand::VisualDedent
            | EditorCommand::VisualToggleCase
            | EditorCommand::VisualUpperCase
            | EditorCommand::VisualLowerCase => self.execute_visual(cmd),

            // Editing
            EditorCommand::Paste
            | EditorCommand::PasteBefore
            | EditorCommand::SwitchMode(..)
            | EditorCommand::InsertChar(..)
            | EditorCommand::InsertTab
            | EditorCommand::DeleteCharBefore
            | EditorCommand::InsertNewline
            | EditorCommand::Undo
            | EditorCommand::Redo
            | EditorCommand::Save
            | EditorCommand::SaveAndQuit
            | EditorCommand::DeleteMotion(..)
            | EditorCommand::ChangeMotion(..)
            | EditorCommand::DeleteLines(..)
            | EditorCommand::ChangeLines(..)
            | EditorCommand::DeleteTextObject(..)
            | EditorCommand::ChangeTextObject(..)
            | EditorCommand::YankTextObject(..)
            | EditorCommand::ReplaceChar(..)
            | EditorCommand::DeleteCharForward
            | EditorCommand::OpenLineBelow
            | EditorCommand::OpenLineAbove
            | EditorCommand::AppendEndOfLine
            | EditorCommand::InsertStartOfLine
            | EditorCommand::ChangeToEnd
            | EditorCommand::DeleteToEnd
            | EditorCommand::SubstituteLine
            | EditorCommand::JoinLines(..)
            | EditorCommand::Indent(..)
            | EditorCommand::Dedent(..)
            | EditorCommand::ToggleCase
            | EditorCommand::ReplaceInsertChar(..)
            | EditorCommand::IncrementNumber
            | EditorCommand::DecrementNumber
            | EditorCommand::UpperCaseMotion(..)
            | EditorCommand::LowerCaseMotion(..)
            | EditorCommand::UpperCaseLine
            | EditorCommand::LowerCaseLine
            | EditorCommand::ReadFile(..)
            | EditorCommand::ReadCommand(..)
            | EditorCommand::SortLines
            | EditorCommand::PipeToCommand(..)
            | EditorCommand::AutoIndent
            | EditorCommand::FormatMotion(..)
            | EditorCommand::FormatLine => self.execute_editing(cmd, screen_area),

            // Panes
            EditorCommand::SplitPane(..)
            | EditorCommand::FocusDirection(..)
            | EditorCommand::FocusNext
            | EditorCommand::ClosePane
            | EditorCommand::OpenTerminal
            | EditorCommand::ForwardToTerminal(..)
            | EditorCommand::ResizePaneGrow
            | EditorCommand::ResizePaneShrink
            | EditorCommand::ResizePaneWider
            | EditorCommand::ResizePaneNarrower
            | EditorCommand::ZoomPane
            | EditorCommand::SwapPane
            | EditorCommand::EnterCopyMode
            | EditorCommand::ExitCopyMode => self.execute_pane(cmd, screen_area),

            // Command mode
            EditorCommand::CommandInput(..)
            | EditorCommand::CommandBackspace
            | EditorCommand::CommandExecute
            | EditorCommand::CommandCancel
            | EditorCommand::CommandHistoryUp
            | EditorCommand::CommandHistoryDown
            | EditorCommand::CommandTab
            | EditorCommand::CommandTabBack => self.execute_command_mode(cmd, screen_area),

            // Finder
            EditorCommand::OpenFileFinder
            | EditorCommand::OpenFinderAt(..)
            | EditorCommand::FinderInput(..)
            | EditorCommand::FinderBackspace
            | EditorCommand::FinderUp
            | EditorCommand::FinderDown
            | EditorCommand::FinderAccept
            | EditorCommand::FinderDismiss => self.execute_finder(cmd),

            // Workspace / tabs
            EditorCommand::OpenTab(..)
            | EditorCommand::NextTab
            | EditorCommand::PrevTab
            | EditorCommand::CloseTab
            | EditorCommand::JumpToTab(..)
            | EditorCommand::ListWorkspaces
            | EditorCommand::RenameTab(..) => self.execute_workspace(cmd, screen_area),

            // LSP commands
            EditorCommand::ShowHover
            | EditorCommand::GotoDefinition
            | EditorCommand::TriggerCompletion
            | EditorCommand::CompletionUp
            | EditorCommand::CompletionDown
            | EditorCommand::CompletionAccept
            | EditorCommand::CompletionDismiss => self.execute_lsp_commands(cmd),

            // Explorer
            EditorCommand::ToggleExplorer
            | EditorCommand::OpenExplorer(..)
            | EditorCommand::FocusExplorer
            | EditorCommand::ExplorerUp
            | EditorCommand::ExplorerDown
            | EditorCommand::ExplorerOpen
            | EditorCommand::ToggleHelp
            | EditorCommand::DismissPopup => self.execute_explorer(cmd),

            // Search
            EditorCommand::EnterSearch
            | EditorCommand::SearchInput(..)
            | EditorCommand::SearchBackspace
            | EditorCommand::SearchExecute
            | EditorCommand::SearchCancel
            | EditorCommand::NextMatch
            | EditorCommand::PrevMatch
            | EditorCommand::ClearSearch
            | EditorCommand::ReplaceAll(..)
            | EditorCommand::ReplaceConfirm(..)
            | EditorCommand::ReplaceConfirmYes
            | EditorCommand::ReplaceConfirmNo
            | EditorCommand::ReplaceConfirmAll
            | EditorCommand::ReplaceConfirmQuit
            | EditorCommand::SearchWordForward
            | EditorCommand::SearchWordBackward => self.execute_search(cmd),

            // Scroll / buffer
            EditorCommand::ScrollUp
            | EditorCommand::ScrollDown
            | EditorCommand::ScrollCenter
            | EditorCommand::ScrollTop
            | EditorCommand::ScrollBottom
            | EditorCommand::ScrollLineDown
            | EditorCommand::ScrollLineUp
            | EditorCommand::ScreenTop
            | EditorCommand::ScreenMiddle
            | EditorCommand::ScreenBottom
            | EditorCommand::PageDown
            | EditorCommand::PageUp
            | EditorCommand::BufferNext
            | EditorCommand::BufferPrev
            | EditorCommand::BufferList
            | EditorCommand::SetOption(..) => self.execute_scroll_buffer(cmd, screen_area),

            // Macros
            EditorCommand::StartMacroRecord(..)
            | EditorCommand::StopMacroRecord
            | EditorCommand::ReplayMacro(..) => self.execute_macros(cmd, screen_area),

            // Marks / jumps
            EditorCommand::SetMark(..)
            | EditorCommand::JumpToMark(..)
            | EditorCommand::JumpBack
            | EditorCommand::JumpForward
            | EditorCommand::JumpToLastPosition
            | EditorCommand::ListMarks => self.execute_marks_jumps(cmd),

            EditorCommand::ListRegisters => {
                let mut items: Vec<String> = self.registers.iter()
                    .map(|(k, v)| {
                        let preview: String = v.chars().take(30).collect();
                        format!("\"{}  {}", k, preview)
                    })
                    .collect();
                items.sort();
                if items.is_empty() {
                    self.status_message = Some("No registers".to_string());
                } else {
                    self.status_message = Some(items.join(" | "));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ReselectVisual => {
                if let Some(sel) = self.last_visual_selection {
                    self.focused_buf_mut().set_selection(Some(sel));
                    self.focused_buf_mut().set_cursor_pos(sel.head);
                    self.mode = EditorMode::Visual;
                } else {
                    self.status_message = Some("No previous visual selection".to_string());
                }
                Ok(ExecOutcome::Continue)
            }

            // Kept inline
            EditorCommand::Quit => self.handle_quit(),
            EditorCommand::ForceQuit => Ok(ExecOutcome::Quit),
            EditorCommand::SaveSession(name) => self.handle_save_session(name),
            EditorCommand::EditFile(path) => {
                self.push_jump();
                self.handle_edit_file(path)
            }
            EditorCommand::DotRepeat => {
                if let Some(edit) = self.last_edit.clone() {
                    // Replay the edit command
                    self.execute(edit.command.clone(), screen_area)?;
                    // If it had insert text, replay those too
                    if !edit.insert_text.is_empty() {
                        for insert_cmd in &edit.insert_text {
                            self.execute(insert_cmd.clone(), screen_area)?;
                        }
                        // Return to normal mode
                        self.execute(EditorCommand::SwitchMode(EditorMode::Normal), screen_area)?;
                    }
                    // Restore the edit record (replay shouldn't overwrite it)
                    self.last_edit = Some(edit);
                } else {
                    self.status_message = Some("No last edit to repeat".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SelectRegister(c) => {
                self.pending_register = Some(*c);
                self.status_message = Some(format!("\"{}",c));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::YankLines(n) => {
                // Yank N lines starting at cursor (without deleting)
                let n = *n;
                let buf = self.focused_buf();
                let start = buf.cursor().line;
                let mut text = String::new();
                for i in start..start + n {
                    if let Some(line) = buf.get_line(i) {
                        text.push_str(&line);
                        text.push('\n');
                    }
                }
                if !text.is_empty() {
                    self.yank_to_register(&text);
                    self.status_message = Some(format!("{} line(s) yanked", n));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::YankToEnd => {
                let buf = self.focused_buf();
                let cursor = buf.cursor();
                if let Some(line) = buf.get_line(cursor.line) {
                    let text: String = line.chars().skip(cursor.column).collect();
                    if !text.is_empty() {
                        self.yank_to_register(&text);
                        self.status_message = Some("Yanked to EOL".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::YankMotion(_dir, _n) => {
                // For now, yank = copy the text that would be deleted by the motion
                // This is a simplified version — just yank the current line for most motions
                let buf = self.focused_buf();
                let cursor = buf.cursor();
                if let Some(line) = buf.get_line(cursor.line) {
                    self.yank_to_register(&line);
                    self.status_message = Some("Yanked".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Echo(msg) => {
                self.status_message = Some(msg.clone());
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::FileInfo => {
                let buf = self.focused_buf();
                let name = buf.display_name();
                let total = buf.len_lines();
                let cursor = buf.cursor();
                let pct = if total > 0 { (cursor.line + 1) * 100 / total } else { 0 };
                let dirty = if buf.is_dirty() { " [Modified]" } else { "" };
                self.status_message = Some(format!(
                    "\"{}\"{} {} lines --{}%--",
                    name, dirty, total, pct
                ));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ChangeDirectory(dir) => {
                let path = std::path::Path::new(dir.as_str());
                if path.is_dir() {
                    if let Err(e) = std::env::set_current_dir(path) {
                        self.status_message = Some(format!("cd: {}", e));
                    } else {
                        self.status_message = Some(format!("cd: {}", dir));
                    }
                } else {
                    self.status_message = Some(format!("cd: not a directory: {}", dir));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PluginList => {
                let list = self.plugins.list();
                if list.is_empty() {
                    self.status_message = Some("No plugins loaded".to_string());
                } else {
                    let items: Vec<String> = list.iter().map(|(id, name, enabled, builtin)| {
                        let status = if *enabled { "+" } else { "-" };
                        let kind = if *builtin { "builtin" } else { "user" };
                        format!("[{}] {} ({}, {})", status, id, name, kind)
                    }).collect();
                    self.status_message = Some(items.join(" | "));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PluginCommand(name, args) => {
                if self.plugins.has_command(name) {
                    let snapshot = self.make_buffer_snapshot();
                    let event = crate::plugin::EditorEvent::CommandExecuted {
                        command: if args.is_empty() { name.clone() } else { format!("{} {}", name, args) },
                    };
                    let actions = self.plugins.dispatch(&event, &snapshot);
                    self.run_plugin_actions(actions, screen_area);
                    Ok(ExecOutcome::Continue)
                } else {
                    self.status_message = Some(format!("Unknown command: {}", name));
                    Ok(ExecOutcome::Continue)
                }
            }
            EditorCommand::SourceFile(path) => {
                let resolved = if path.starts_with('/') || path.starts_with('~') {
                    let p = path.replace('~', &std::env::var("HOME").unwrap_or_default());
                    std::path::PathBuf::from(p)
                } else {
                    self.tabs[self.active_tab].shell_cwd().join(path)
                };
                self.plugins.reload_file(&resolved);
                self.status_message = Some(format!("Reloaded: {}", resolved.display()));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ShellCommand(cmd) => {
                match std::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let result = if !stdout.is_empty() {
                            stdout.trim().to_string()
                        } else if !stderr.is_empty() {
                            format!("Error: {}", stderr.trim())
                        } else {
                            "Command completed".to_string()
                        };
                        self.status_message = Some(result);
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Shell error: {}", e));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenUrlUnderCursor => {
                let cursor = self.focused_buf().cursor();
                if let Some(line) = self.focused_buf().get_line(cursor.line) {
                    if let Some(url) = crate::url::url_at_position(&line, cursor.column) {
                        crate::url::open_url(&url);
                        self.status_message = Some(format!("Opening: {}", url));
                    } else {
                        self.status_message = Some("No URL at cursor".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::OpenFileUnderCursor => {
                if let Some(word) = self.focused_buf().word_at_cursor() {
                    let idx = self.active_tab;
                    let base = self.tabs[idx].shell_cwd();
                    let path = if std::path::Path::new(&word).is_absolute() {
                        std::path::PathBuf::from(&word)
                    } else {
                        base.join(&word)
                    };
                    if path.exists() && path.is_file() {
                        self.push_jump();
                        let path_str = path.to_string_lossy().to_string();
                        return self.handle_edit_file(&path_str);
                    } else {
                        self.status_message = Some(format!("File not found: {}", word));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            // Quickfix
            EditorCommand::QuickfixOpen => {
                if self.quickfix.entries.is_empty() {
                    // Populate from LSP diagnostics
                    let idx = self.active_tab;
                    let mut entries = Vec::new();
                    for (uri, diags) in &self.tabs[idx].diagnostics {
                        for d in diags {
                            entries.push(QuickfixEntry {
                                file: uri.clone(),
                                line: d.line + 1,
                                col: d.col_start + 1,
                                message: d.message.clone(),
                            });
                        }
                    }
                    entries.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
                    self.quickfix.entries = entries;
                    self.quickfix.current = 0;
                }
                if self.quickfix.entries.is_empty() {
                    self.status_message = Some("Quickfix list is empty".to_string());
                } else {
                    self.quickfix.visible = true;
                    let e = &self.quickfix.entries[self.quickfix.current];
                    self.status_message = Some(format!(
                        "[{}/{}] {}:{}:{}: {}",
                        self.quickfix.current + 1, self.quickfix.entries.len(),
                        e.file, e.line, e.col, e.message
                    ));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::QuickfixNext => {
                if !self.quickfix.entries.is_empty() {
                    self.quickfix.current = (self.quickfix.current + 1).min(self.quickfix.entries.len() - 1);
                    let e = self.quickfix.entries[self.quickfix.current].clone();
                    self.push_jump();
                    let _ = self.handle_edit_file(&e.file);
                    self.focused_buf_mut().set_cursor_pos(
                        novim_types::Position::new(e.line.saturating_sub(1), e.col.saturating_sub(1)),
                    );
                    self.status_message = Some(format!(
                        "[{}/{}] {}",
                        self.quickfix.current + 1, self.quickfix.entries.len(), e.message
                    ));
                } else {
                    self.status_message = Some("Quickfix list is empty".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::QuickfixPrev => {
                if !self.quickfix.entries.is_empty() {
                    self.quickfix.current = self.quickfix.current.saturating_sub(1);
                    let e = self.quickfix.entries[self.quickfix.current].clone();
                    self.push_jump();
                    let _ = self.handle_edit_file(&e.file);
                    self.focused_buf_mut().set_cursor_pos(
                        novim_types::Position::new(e.line.saturating_sub(1), e.col.saturating_sub(1)),
                    );
                    self.status_message = Some(format!(
                        "[{}/{}] {}",
                        self.quickfix.current + 1, self.quickfix.entries.len(), e.message
                    ));
                } else {
                    self.status_message = Some("Quickfix list is empty".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::QuickfixClose => {
                self.quickfix.visible = false;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::Make(cmd) => {
                self.status_message = Some(format!(":make {} ...", cmd));
                let cmd = cmd.clone();
                self.tasks.spawn(move || {
                    match std::process::Command::new("sh")
                        .args(["-c", &cmd])
                        .output()
                    {
                        Ok(output) => {
                            let text = String::from_utf8_lossy(&output.stderr);
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let combined = if !text.is_empty() { text } else { stdout };
                            let entries: Vec<QuickfixEntry> = combined.lines()
                                .filter_map(parse_quickfix_line)
                                .collect();
                            if entries.is_empty() && !output.status.success() {
                                vec![crate::async_task::TaskResult::StatusMessage(
                                    format!(":make — exit {}", output.status),
                                )]
                            } else {
                                vec![crate::async_task::TaskResult::QuickfixEntries(entries)]
                            }
                        }
                        Err(e) => {
                            vec![crate::async_task::TaskResult::StatusMessage(
                                format!(":make error: {}", e),
                            )]
                        }
                    }
                });
                Ok(ExecOutcome::Continue)
            }
            // Command history window
            EditorCommand::OpenCommandWindow => {
                if self.command_history.is_empty() {
                    self.status_message = Some("Command history is empty".to_string());
                } else {
                    self.command_window.visible = true;
                    self.command_window.selected = self.command_history.len().saturating_sub(1);
                    self.status_message = Some("q: — Enter to execute, q/Esc to close".to_string());
                }
                Ok(ExecOutcome::Continue)
            }

            // Symbol list
            EditorCommand::SymbolList => {
                let buf = self.focused_buf();
                let total = buf.len_lines();
                let mut source = String::new();
                for i in 0..total {
                    if let Some(line) = buf.get_line(i) {
                        source.push_str(&line);
                        source.push('\n');
                    }
                }
                let ext = buf.display_name();
                let ext = std::path::Path::new(&ext)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if let Some(lang) = crate::highlight::Language::from_extension(ext) {
                    let symbols = crate::highlight::extract_symbols(&source, lang);
                    if symbols.is_empty() {
                        self.status_message = Some("No symbols found".to_string());
                    } else {
                        let count = symbols.len();
                        self.symbol_list.symbols = symbols;
                        self.symbol_list.query.clear();
                        self.symbol_list.filter();
                        self.symbol_list.selected = 0;
                        self.symbol_list.visible = true;
                        self.status_message = Some(format!("{} symbols", count));
                    }
                } else {
                    self.status_message = Some("No tree-sitter support for this file type".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolUp => {
                if self.symbol_list.selected > 0 {
                    self.symbol_list.selected -= 1;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolDown => {
                if self.symbol_list.selected + 1 < self.symbol_list.filtered.len() {
                    self.symbol_list.selected += 1;
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolAccept => {
                if let Some(&idx) = self.symbol_list.filtered.get(self.symbol_list.selected) {
                    let line = self.symbol_list.symbols[idx].line;
                    self.push_jump();
                    self.focused_buf_mut().set_cursor_pos(novim_types::Position::new(line, 0));
                }
                self.symbol_list.visible = false;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolDismiss => {
                self.symbol_list.visible = false;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolInput(c) => {
                self.symbol_list.query.push(*c);
                self.symbol_list.filter();
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SymbolBackspace => {
                self.symbol_list.query.pop();
                self.symbol_list.filter();
                Ok(ExecOutcome::Continue)
            }
            // Floating windows
            EditorCommand::OpenFloat { title, lines, width, height } => {
                self.floating_windows.push(FloatingWindow {
                    title: title.clone(),
                    lines: lines.clone(),
                    width: *width,
                    height: *height,
                    scroll: 0,
                    selected: 0,
                });
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::CloseFloat => {
                self.floating_windows.pop();
                Ok(ExecOutcome::Continue)
            }

            // Git blame
            EditorCommand::ToggleBlame => {
                let idx = self.active_tab;
                let pane = self.tabs[idx].panes.focused_pane_mut();
                if let crate::pane::PaneContent::Editor(buf) = &mut pane.content {
                    if buf.show_blame {
                        buf.show_blame = false;
                        buf.blame_info.clear();
                        self.status_message = Some("Blame off".to_string());
                    } else if let Some(path) = buf.file_path_str().map(|s| s.to_string()) {
                        // Run blame on background thread to avoid blocking UI
                        self.status_message = Some("Computing blame...".to_string());
                        self.tasks.spawn(move || {
                            let blame = crate::git::compute_blame(&std::path::PathBuf::from(&path));
                            if blame.is_empty() {
                                vec![crate::async_task::TaskResult::StatusMessage(
                                    "No blame data (not in git or uncommitted)".to_string(),
                                )]
                            } else {
                                vec![crate::async_task::TaskResult::BlameData(blame)]
                            }
                        });
                    } else {
                        self.status_message = Some("No file path for blame".to_string());
                    }
                } else {
                    self.status_message = Some("Blame only works on editor buffers".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            // Git diff
            EditorCommand::GitDiff => {
                let idx = self.active_tab;
                let file_path = {
                    let pane = self.tabs[idx].panes.focused_pane();
                    match &pane.content {
                        crate::pane::PaneContent::Editor(buf) => buf.file_path_str().map(|s| s.to_string()),
                        _ => None,
                    }
                };
                if let Some(path) = file_path {
                    let path_buf = std::path::PathBuf::from(&path);
                    if let Some(head_content) = crate::git::head_file_content(&path_buf) {
                        let name = std::path::Path::new(&path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("diff")
                            .to_string();

                        // Get working copy text for diff computation
                        let working_text = {
                            let buf = self.focused_buf();
                            let total = buf.len_lines();
                            let mut t = String::new();
                            for i in 0..total {
                                if let Some(line) = buf.get_line(i) {
                                    t.push_str(&line);
                                    t.push('\n');
                                }
                            }
                            t
                        };

                        // Compute diff
                        let (old_diff, new_diff) = crate::git::compute_line_diff(&head_content, &working_text);

                        // Apply diff highlights to working copy (current buffer)
                        if let crate::pane::PaneContent::Editor(buf) = &mut self.tabs[idx].panes.focused_pane_mut().content {
                            buf.diff_lines = new_diff;
                        }

                        // Create HEAD buffer from string with syntax highlighting
                        let ext = std::path::Path::new(&path)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        let mut head_buf = crate::buffer::Buffer::from_string_with_ext(&head_content, ext);
                        head_buf.display_label = Some(format!("{} (HEAD)", name));
                        head_buf.diff_lines = old_diff;

                        // Open in a vertical split
                        let _new_pane_id = self.tabs[idx].panes.split_with_buffer(
                            crate::pane::SplitDirection::Vertical,
                            head_buf,
                        );
                        self.status_message = Some(format!("Diff: {} (HEAD) | {} (working)", name, name));
                    } else {
                        self.status_message = Some("No HEAD version found (file not committed?)".to_string());
                    }
                } else {
                    self.status_message = Some("No file path for diff".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ToggleOutline => {
                if self.outline.visible {
                    self.outline.visible = false;
                    self.outline.symbols.clear();
                    self.status_message = Some("Outline off".to_string());
                } else {
                    self.refresh_outline();
                    if self.outline.symbols.is_empty() {
                        self.status_message = Some("No symbols found".to_string());
                    } else {
                        self.outline.visible = true;
                        self.status_message = Some(format!("Outline: {} symbols", self.outline.symbols.len()));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SetColorScheme(name) => {
                if name.is_empty() {
                    // List available themes
                    let themes = crate::theme::available_themes();
                    let current = self.config.colorscheme.as_deref().unwrap_or("default");
                    let list: Vec<String> = themes.iter().map(|t| {
                        if t == current { format!("*{}", t) } else { t.clone() }
                    }).collect();
                    self.status_message = Some(format!("Themes: {}", list.join(" | ")));
                } else if let Some(scheme) = crate::theme::load_theme(name) {
                    self.config.theme = scheme.to_theme_config();
                    self.config.syntax_theme = scheme.to_syntax_theme();
                    self.config.colorscheme = Some(name.clone());
                    // Persist to config file
                    if let Err(e) = persist_colorscheme(name) {
                        self.status_message = Some(format!("Theme applied but save failed: {}", e));
                    } else {
                        self.status_message = Some(format!("Colorscheme: {}", name));
                    }
                    // Force re-highlight all buffers
                    let idx = self.active_tab;
                    if let crate::pane::PaneContent::Editor(buf) = &mut self.tabs[idx].panes.focused_pane_mut().content {
                        buf.force_rehighlight();
                    }
                } else {
                    self.status_message = Some(format!("Unknown colorscheme: {}", name));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::ToggleMinimap => {
                self.config.editor.minimap = !self.config.editor.minimap;
                self.status_message = Some(format!(
                    "Minimap {}",
                    if self.config.editor.minimap { "on" } else { "off" }
                ));
                Ok(ExecOutcome::Continue)
            }
            // Plugin package manager
            EditorCommand::PluginInstall(url) => {
                let plugins_dir = plugin_install_dir();
                // Derive name from URL: https://github.com/user/repo.git → repo
                let name = url.rsplit('/')
                    .next()
                    .unwrap_or("plugin")
                    .trim_end_matches(".git")
                    .to_string();
                let dest = plugins_dir.join(&name);
                if dest.exists() {
                    self.status_message = Some(format!("Plugin '{}' already installed. Use :PlugUpdate", name));
                    return Ok(ExecOutcome::Continue);
                }
                std::fs::create_dir_all(&plugins_dir).ok();
                match std::process::Command::new("git")
                    .args(["clone", "--depth", "1", url, &dest.to_string_lossy()])
                    .output()
                {
                    Ok(output) => {
                        if output.status.success() {
                            // Try to load the new plugin immediately
                            let manifest_path = dest.join("plugin.toml");
                            let manifest = crate::plugin::PluginManifest::from_file(&manifest_path);
                            let entry_file = manifest.as_ref()
                                .and_then(|m| m.entry.as_deref())
                                .unwrap_or("init.lua");
                            let lua_path = dest.join(entry_file);
                            if lua_path.is_file() {
                                self.status_message = Some(format!("Installed '{}'. Restart to load, or :source {}", name, lua_path.display()));
                            } else {
                                self.status_message = Some(format!("Installed '{}' (no init.lua found)", name));
                            }
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            self.status_message = Some(format!("git clone failed: {}", stderr.trim()));
                        }
                    }
                    Err(e) => {
                        self.status_message = Some(format!("git clone error: {}", e));
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PluginUpdate(name) => {
                let plugins_dir = plugin_install_dir();
                let dirs_to_update: Vec<PathBuf> = if name.is_empty() {
                    // Update all directory-based plugins
                    std::fs::read_dir(&plugins_dir)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().is_dir() && e.path().join(".git").is_dir())
                        .map(|e| e.path())
                        .collect()
                } else {
                    let p = plugins_dir.join(&*name);
                    if p.is_dir() { vec![p] } else { Vec::new() }
                };
                if dirs_to_update.is_empty() {
                    self.status_message = Some(if name.is_empty() {
                        "No git-based plugins to update".to_string()
                    } else {
                        format!("Plugin '{}' not found", name)
                    });
                    return Ok(ExecOutcome::Continue);
                }
                let mut updated = 0;
                let mut errors = Vec::new();
                for dir in &dirs_to_update {
                    match std::process::Command::new("git")
                        .args(["pull", "--ff-only"])
                        .current_dir(dir)
                        .output()
                    {
                        Ok(output) => {
                            if output.status.success() {
                                updated += 1;
                            } else {
                                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                                errors.push(name.to_string());
                            }
                        }
                        Err(_) => {
                            let name = dir.file_name().unwrap_or_default().to_string_lossy();
                            errors.push(name.to_string());
                        }
                    }
                }
                if errors.is_empty() {
                    self.status_message = Some(format!("Updated {} plugin(s). Restart to reload.", updated));
                } else {
                    self.status_message = Some(format!("Updated {}, failed: {}", updated, errors.join(", ")));
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PluginRemove(name) => {
                let plugins_dir = plugin_install_dir();
                let dest = plugins_dir.join(&*name);
                if !dest.is_dir() {
                    self.status_message = Some(format!("Plugin '{}' not found", name));
                    return Ok(ExecOutcome::Continue);
                }
                match std::fs::remove_dir_all(&dest) {
                    Ok(_) => {
                        self.status_message = Some(format!("Removed '{}'. Restart to unload.", name));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to remove '{}': {}", name, e));
                    }
                }
                Ok(ExecOutcome::Continue)
            }

            EditorCommand::Noop => Ok(ExecOutcome::Continue),
            EditorCommand::ForceRedraw => Ok(ExecOutcome::Continue),
        }
    }
}

/// Resolve a path relative to the focused pane's working directory.
pub fn resolve_path(path: &str, panes: &PaneManager, last_shell_cwd: Option<&PathBuf>) -> PathBuf {
    let base = panes.any_terminal_shell_cwd()
        .or_else(|| last_shell_cwd.cloned())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    if path == "." {
        base
    } else {
        let p = PathBuf::from(path);
        if p.is_absolute() { p } else {
            base.join(p)
        }
    }
}

impl EditorState {
    /// Poll background tasks and apply results.
    pub fn poll_tasks(&mut self) {
        let results = self.tasks.poll();
        for result in results {
            match result {
                crate::async_task::TaskResult::StatusMessage(msg) => {
                    self.status_message = Some(msg);
                }
                crate::async_task::TaskResult::BlameData(blame) => {
                    let idx = self.active_tab;
                    let pane = self.tabs[idx].panes.focused_pane_mut();
                    if let crate::pane::PaneContent::Editor(buf) = &mut pane.content {
                        let count = blame.len();
                        buf.blame_info = blame;
                        buf.show_blame = true;
                        self.status_message = Some(format!("Blame: {} lines", count));
                    }
                }
                crate::async_task::TaskResult::QuickfixEntries(entries) => {
                    let count = entries.len();
                    self.quickfix.entries = entries;
                    self.quickfix.current = 0;
                    if count > 0 {
                        self.quickfix.visible = true;
                        self.status_message = Some(format!(":make — {} error(s)", count));
                    } else {
                        self.status_message = Some(":make — success".to_string());
                    }
                }
                crate::async_task::TaskResult::GitBranch(branch) => {
                    self.git_branch = branch;
                }
            }
        }
    }

    /// Refresh the outline symbols from the current buffer.
    pub fn refresh_outline(&mut self) {
        let buf = self.focused_buf();
        let total = buf.len_lines();
        let mut source = String::new();
        for i in 0..total {
            if let Some(line) = buf.get_line(i) {
                source.push_str(&line);
                source.push('\n');
            }
        }
        let ext = buf.display_name();
        let ext = std::path::Path::new(&ext)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if let Some(lang) = crate::highlight::Language::from_extension(ext) {
            self.outline.symbols = crate::highlight::extract_symbols(&source, lang);
        } else {
            self.outline.symbols.clear();
        }
        self.update_breadcrumb();
    }

    /// Update the breadcrumb string based on cursor position and outline symbols.
    pub fn update_breadcrumb(&mut self) {
        if self.outline.symbols.is_empty() {
            self.outline.breadcrumb.clear();
            return;
        }
        let cursor_line = self.focused_buf().cursor().line;
        let trail = crate::highlight::breadcrumb_at(&self.outline.symbols, cursor_line);
        self.outline.breadcrumb = trail.iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(" > ");

        // Update selected in outline to match cursor position
        if let Some(closest) = self.outline.symbols.iter().enumerate()
            .filter(|(_, s)| cursor_line >= s.line && cursor_line <= s.end_line)
            .max_by_key(|(_, s)| s.depth)
        {
            self.outline.selected = closest.0;
        }
    }
}

/// Persist the colorscheme choice to config.toml.
fn persist_colorscheme(name: &str) -> std::io::Result<()> {
    let Some(path) = crate::config::config_path() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"));
    };

    // Read existing config, update colorscheme field, write back
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();

    if content.contains("colorscheme") {
        // Replace existing colorscheme line
        let mut new_lines = Vec::new();
        for line in content.lines() {
            if line.trim_start().starts_with("colorscheme") {
                new_lines.push(format!("colorscheme = \"{}\"", name));
            } else {
                new_lines.push(line.to_string());
            }
        }
        content = new_lines.join("\n");
    } else {
        // Append at the top
        content = format!("colorscheme = \"{}\"\n{}", name, content);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)
}

/// Plugin install directory: ~/.config/novim/plugins/
fn plugin_install_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("novim").join("plugins")
}

/// Parse a compiler output line into a quickfix entry.
/// Handles formats: file:line:col: message, file:line: message, file(line): message
fn parse_quickfix_line(line: &str) -> Option<QuickfixEntry> {
    // Try file:line:col: message
    let parts: Vec<&str> = line.splitn(4, ':').collect();
    if parts.len() >= 3 {
        if let Ok(line_num) = parts[1].trim().parse::<usize>() {
            if let Ok(col_num) = parts[2].trim().parse::<usize>() {
                let msg = parts.get(3).unwrap_or(&"").trim().to_string();
                if !msg.is_empty() {
                    return Some(QuickfixEntry {
                        file: parts[0].to_string(),
                        line: line_num,
                        col: col_num,
                        message: msg,
                    });
                }
            }
            // file:line: message (no col)
            let msg = parts[2..].join(":").trim().to_string();
            if !msg.is_empty() {
                return Some(QuickfixEntry {
                    file: parts[0].to_string(),
                    line: line_num,
                    col: 1,
                    message: msg,
                });
            }
        }
    }
    None
}

/// Handle a keypress in terminal copy mode. Shared between TUI and GUI.
/// Returns an EditorCommand to execute (ExitCopyMode or Noop).
pub fn handle_copy_mode_key(
    panes: &mut crate::pane::PaneManager,
    focused_id: crate::pane::PaneId,
    key: crossterm::event::KeyEvent,
    registers: &mut HashMap<char, String>,
    status_message: &mut Option<String>,
) -> crate::input::EditorCommand {
    use crossterm::event::KeyCode;
    use crate::input::EditorCommand;

    let Some(pane) = panes.get_pane_mut(focused_id) else {
        return EditorCommand::Noop;
    };

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            pane.copy_mode_selection = None;
            return EditorCommand::ExitCopyMode;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let max = pane.content.as_buffer_like().scrollback_len();
            if pane.copy_mode_offset < max {
                pane.copy_mode_offset += 1;
                pane.copy_mode_cursor.0 += 1;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if pane.copy_mode_offset > 1 {
                pane.copy_mode_offset -= 1;
                pane.copy_mode_cursor.0 = pane.copy_mode_cursor.0.saturating_sub(1);
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            pane.copy_mode_cursor.1 = pane.copy_mode_cursor.1.saturating_sub(1);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            pane.copy_mode_cursor.1 += 1;
        }
        KeyCode::Char('v') => {
            // Toggle selection
            if pane.copy_mode_selection.is_some() {
                pane.copy_mode_selection = None;
                *status_message = Some("Selection cleared".to_string());
            } else {
                pane.copy_mode_selection = Some(pane.copy_mode_cursor);
                *status_message = Some("Selection started (y to yank)".to_string());
            }
        }
        KeyCode::Char('y') => {
            // Yank selected text (or current line if no selection)
            let mut text = String::new();
            let scrollback_len = pane.content.as_buffer_like().scrollback_len();

            if let Some(anchor) = pane.copy_mode_selection {
                // Yank from anchor to cursor
                let (start_row, end_row) = if anchor.0 >= pane.copy_mode_cursor.0 {
                    (pane.copy_mode_cursor.0, anchor.0)
                } else {
                    (anchor.0, pane.copy_mode_cursor.0)
                };

                for row_off in start_row..=end_row {
                    if let Some(cells) = pane.content.as_buffer_like().scrollback_line(row_off) {
                        let line: String = cells.iter().map(|c| c.c).collect();
                        if !text.is_empty() { text.push('\n'); }
                        text.push_str(line.trim_end());
                    } else {
                        // It's on the visible screen
                        let screen_row = scrollback_len.saturating_sub(row_off);
                        if let Some(cells) = pane.content.as_buffer_like().get_styled_cells(screen_row) {
                            let line: String = cells.iter().map(|c| c.c).collect();
                            if !text.is_empty() { text.push('\n'); }
                            text.push_str(line.trim_end());
                        }
                    }
                }
            } else {
                // Yank current line
                let row_off = pane.copy_mode_cursor.0;
                if let Some(cells) = pane.content.as_buffer_like().scrollback_line(row_off) {
                    text = cells.iter().map(|c| c.c).collect::<String>().trim_end().to_string();
                }
            }

            if !text.is_empty() {
                // Store in unnamed register and system clipboard
                registers.insert('"', text.clone());
                if let Ok(mut clip) = arboard::Clipboard::new() {
                    let _ = clip.set_text(text.clone());
                }
                let lines = text.lines().count();
                *status_message = Some(format!("Yanked {} line(s)", lines));
            }

            pane.copy_mode_selection = None;
            pane.copy_mode_offset = 0;
            return EditorCommand::ExitCopyMode;
        }
        _ => {}
    }
    EditorCommand::Noop
}
