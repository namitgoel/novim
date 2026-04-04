//! Editor state machine — shared between TUI and GUI frontends.
//!
//! Contains: EditorState, Workspace, sub-states, command execution.
//! This module has NO dependency on Ratatui or any rendering crate.
//! Input events use crossterm types as the common format.

mod types;
mod workspace;
mod handlers;
mod input;

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

    fn with_config_and_tabs(cfg: NovimConfig, registry: Arc<LspRegistry>, tabs: Vec<Workspace>, active_tab: usize, status_message: Option<String>) -> Self {
        let ln_mode = ln_mode_from_config(&cfg.editor.line_numbers);
        let mut plugins = PluginManager::new(false, std::collections::HashMap::new());
        crate::plugin::builtins::register_builtins(&mut plugins);
        plugins.load_lua_plugins();
        let git_branch = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            });
        let load_errors = plugins.take_load_errors();
        let status_message = if !load_errors.is_empty() {
            Some(format!("Plugin errors: {}", load_errors.join("; ")))
        } else {
            status_message
        };
        Self {
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
        }
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
            | EditorCommand::YankSelection => self.execute_visual(cmd),

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
            | EditorCommand::ReplaceInsertChar(..) => self.execute_editing(cmd, screen_area),

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
            | EditorCommand::CommandHistoryDown => self.execute_command_mode(cmd, screen_area),

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
