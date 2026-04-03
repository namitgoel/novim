//! Editor state machine — shared between TUI and GUI frontends.
//!
//! Contains: EditorState, Workspace, sub-states, command execution.
//! This module has NO dependency on Ratatui or any rendering crate.
//! Input events use crossterm types as the common format.

use crossterm::event::{KeyEvent, MouseEvent, MouseEventKind, MouseButton};

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer::{Buffer, BufferLike};
use crate::config::{self, NovimConfig};
use crate::error::NovimError;
use crate::explorer::Explorer;
use crate::finder;
use crate::highlight;
use crate::input::{key_to_command, parse_ex_command, CountState, EditorCommand, InputState};
use crate::lsp::{self, LspClient, LspEvent};
use crate::lsp::provider::LspRegistry;
use crate::pane::{PaneContent, PaneManager, SplitDirection};
use crate::plugin::manager::PluginManager;
use crate::session;
use novim_types::{EditorMode, Selection};

/// Copy text to the system clipboard (best-effort, silent failure).
fn set_system_clipboard(text: &str) {
    if let Ok(mut clip) = arboard::Clipboard::new() {
        let _ = clip.set_text(text.to_string());
    }
}

/// Read text from the system clipboard.
fn get_system_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Result of executing an editor command.
pub enum ExecOutcome {
    /// Continue the event loop.
    Continue,
    /// Exit the editor.
    Quit,
}

/// Line number display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineNumberMode {
    /// Show absolute line numbers (1, 2, 3...)
    Absolute,
    /// Show distance from cursor (3, 2, 1, 0, 1, 2, 3)
    Relative,
    /// Hybrid: cursor line = absolute, others = relative distance
    Hybrid,
    /// No line numbers
    Off,
}

/// Per-workspace state: panes, explorer, LSP, buffer history.
pub struct Workspace {
    pub name: String,
    pub panes: PaneManager,
    pub explorer: Option<Explorer>,
    pub explorer_focused: bool,
    pub buffer_history: Vec<String>,
    pub buffer_history_idx: usize,
    pub launch_dir: PathBuf,
    /// Cached CWD from the last-seen terminal (survives terminal pane replacement).
    pub last_shell_cwd: Option<PathBuf>,
    // LSP
    lsp_registry: Arc<LspRegistry>,
    pub lsp_clients: HashMap<String, LspClient>,
    pub diagnostics: HashMap<String, Vec<lsp::Diagnostic>>,
    pending_goto: Option<(String, u32, u32)>,
    pub lsp_status: Option<String>,
    pub show_buffer_list: bool,
}

impl Workspace {
    fn new_with(name: &str, panes: PaneManager, registry: Arc<LspRegistry>) -> Self {
        Self {
            name: name.to_string(),
            panes,
            explorer: None,
            explorer_focused: false,
            buffer_history: Vec::new(),
            buffer_history_idx: 0,
            launch_dir: std::env::current_dir().unwrap_or_default(),
            last_shell_cwd: None,
            lsp_registry: registry,
            lsp_clients: HashMap::new(),
            diagnostics: HashMap::new(),
            pending_goto: None,
            lsp_status: None,
            show_buffer_list: false,
        }
    }

    pub fn new_editor(name: &str, registry: Arc<LspRegistry>) -> Self {
        Self::new_with(name, PaneManager::new(Buffer::new()), registry)
    }

    pub fn new_terminal(name: &str, rows: u16, cols: u16, registry: Arc<LspRegistry>) -> io::Result<Self> {
        Ok(Self::new_with(name, PaneManager::new_terminal(rows, cols)?, registry))
    }

    pub fn with_file(name: &str, path: &str, registry: Arc<LspRegistry>) -> io::Result<Self> {
        let mut ws = Self::new_with(name, PaneManager::new(Buffer::from_file(path)?), registry);
        ws.buffer_history = vec![path.to_string()];
        Ok(ws)
    }

    pub fn new_terminal_at(name: &str, dir: &Path, registry: Arc<LspRegistry>, screen_area: novim_types::Rect) -> Self {
        let rows = (screen_area.height / 2).max(5);
        let cols = screen_area.width.saturating_sub(2);
        let panes = PaneManager::new_terminal(rows, cols).unwrap_or_else(|_| PaneManager::new(Buffer::new()));
        let mut ws = Self::new_with(name, panes, registry);
        ws.launch_dir = dir.to_path_buf();
        ws
    }

    pub fn from_session(name: &str, panes: PaneManager, registry: Arc<LspRegistry>) -> Self {
        Self::new_with(name, panes, registry)
    }

    /// Poll all LSP clients for events (diagnostics, goto responses).
    pub fn poll_lsp(&mut self) {
        let client_ids: Vec<String> = self.lsp_clients.keys().cloned().collect();
        for lang_id in client_ids {
            if let Some(client) = self.lsp_clients.get_mut(&lang_id) {
                let events = client.poll();
                for event in events {
                    match event {
                        LspEvent::Diagnostics { uri, diagnostics } => {
                            self.diagnostics.insert(uri, diagnostics);
                        }
                        LspEvent::GotoDefinitionResponse { uri, line, col } => {
                            self.pending_goto = Some((uri, line, col));
                        }
                        LspEvent::HoverResponse { contents } => {
                            self.lsp_status = Some(format!("hover:{}", contents));
                        }
                        LspEvent::CompletionResponse { items } => {
                            if !items.is_empty() {
                                self.lsp_status = Some(format!("completion:{}", items.len()));
                            }
                        }
                        LspEvent::ServerError(msg) => {
                            self.lsp_status = Some(format!("LSP: {}", msg));
                        }
                        LspEvent::Progress { message } => {
                            self.lsp_status = Some(message);
                        }
                        LspEvent::ServerExited => {
                            self.lsp_status = Some("exited".to_string());
                        }
                        LspEvent::Initialized => {
                            self.lsp_status = Some("Ready".to_string());
                        }
                    }
                }
            }
        }
    }

    /// Poll terminal panes for output.
    pub fn poll_terminals(&mut self) -> bool {
        let changed = self.panes.poll_terminals();
        // Keep the cached shell CWD up-to-date while terminals are alive.
        if let Some(cwd) = self.panes.any_terminal_shell_cwd() {
            self.last_shell_cwd = Some(cwd);
        }
        changed
    }

    /// Best-effort shell CWD: live terminal → cached → launch_dir.
    pub fn shell_cwd(&self) -> PathBuf {
        self.panes.any_terminal_shell_cwd()
            .or_else(|| self.last_shell_cwd.clone())
            .unwrap_or_else(|| self.launch_dir.clone())
    }

    /// Resize all terminal panes in this workspace.
    pub fn resize_terminals(&mut self, rows: u16, cols: u16) {
        self.panes.resize_terminals(rows, cols);
    }

    /// Ensure an LSP client exists for the focused buffer's language.
    pub fn ensure_lsp_for_buffer(&mut self, lsp_enabled: bool) {
        if !lsp_enabled {
            return;
        }

        let pane = self.panes.focused_pane();
        let buf = pane.content.as_buffer_like();
        if buf.is_terminal() {
            return;
        }

        let ext = match &pane.content {
            PaneContent::Editor(b) => {
                b.file_path_str()
                    .and_then(|p| p.rsplit('.').next())
                    .unwrap_or("")
                    .to_string()
            }
            _ => return,
        };
        let ext = ext.as_str();
        if ext.is_empty() || ext == "[No Name]" {
            return;
        }

        if let Some(server) = self.lsp_registry.resolve(ext) {
            if self.lsp_clients.contains_key(&server.language_id) {
                if let Some(uri) = self.get_focused_uri() {
                    let lang_id = server.language_id.clone();
                    let text = self.get_focused_text();
                    let version = self.get_focused_version();
                    if let Some(client) = self.lsp_clients.get_mut(&lang_id) {
                        let _ = client.did_open(&uri, &lang_id, version, &text);
                    }
                }
                return;
            }

            let root = std::env::current_dir().unwrap_or_default();
            match LspClient::spawn(&server, &root) {
                Ok(mut client) => {
                    if let Some(uri) = self.get_focused_uri() {
                        let text = self.get_focused_text();
                        let version = self.get_focused_version();
                        let _ = client.did_open(&uri, &server.language_id, version, &text);
                    }
                    let lang_id = server.language_id.clone();
                    self.lsp_clients.insert(lang_id, client);
                }
                Err(_e) => {}
            }
        }
    }

    /// Send didChange to the appropriate LSP client for the focused buffer.
    pub fn notify_lsp_change(&mut self) {
        if let Some(uri) = self.get_focused_uri() {
            let text = self.get_focused_text();
            let version = self.get_focused_version();

            let display = self.panes.focused_pane().content.as_buffer_like().display_name();
            let ext = display.rsplit('.').next().unwrap_or("");
            if let Some(server) = self.lsp_registry.resolve(ext) {
                if let Some(client) = self.lsp_clients.get_mut(&server.language_id) {
                    let _ = client.did_change(&uri, version, &text);
                }
            }
        }
    }

    fn get_focused_uri(&self) -> Option<String> {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.file_uri(),
            _ => None,
        }
    }

    fn get_focused_text(&self) -> String {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.full_text(),
            _ => String::new(),
        }
    }

    fn get_focused_version(&self) -> i32 {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.version(),
            _ => 0,
        }
    }
}

/// Search-related UI state.
#[derive(Default)]
pub struct SearchState {
    pub active: bool,
    pub buffer: String,
    pub pattern: Option<String>,
}

/// File finder popup state.
pub struct FinderState {
    pub visible: bool,
    pub query: String,
    pub results: Vec<finder::FileMatch>,
    pub selected: usize,
    pub root: PathBuf,
    pub preview_lines: Vec<String>,
    pub preview_highlights: Vec<Vec<highlight::HighlightSpan>>,
}

impl Default for FinderState {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            root: std::env::current_dir().unwrap_or_default(),
            preview_lines: Vec::new(),
            preview_highlights: Vec::new(),
        }
    }
}

/// LSP completion popup state.
#[derive(Default)]
pub struct CompletionState {
    pub visible: bool,
    pub items: Vec<lsp::CompletionItem>,
    pub selected: usize,
}

/// Macro recording/playback state.
#[derive(Default)]
pub struct MacroState {
    pub registers: HashMap<char, Vec<KeyEvent>>,
    pub recording: Option<char>,
    pub buffer: Vec<KeyEvent>,
    pub last_register: Option<char>,
}

/// Plugin popup state.
pub struct PluginPopup {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub selected: usize,
    pub width: Option<u16>,
    pub height: Option<u16>,
    /// Plugin ID + callback key for on_select. None = display-only popup.
    pub on_select: Option<(String, String)>,
}

/// Record of an edit operation for dot repeat.
#[derive(Debug, Clone)]
pub struct EditRecord {
    /// The initial edit command (e.g. DeleteLines, ChangeMotion, etc.)
    pub command: EditorCommand,
    /// Text typed during insert mode (for change commands).
    pub insert_text: Vec<EditorCommand>,
}

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
    recording_insert: bool,
    /// Accumulated insert-mode keystrokes for dot repeat.
    insert_record: Vec<EditorCommand>,
    /// Plugin system manager.
    pub plugins: PluginManager,
}

fn ln_mode_from_config(s: &str) -> LineNumberMode {
    match s {
        "absolute" | "number" => LineNumberMode::Absolute,
        "relative" => LineNumberMode::Relative,
        "hybrid" => LineNumberMode::Hybrid,
        "off" | "none" => LineNumberMode::Off,
        _ => LineNumberMode::Hybrid,
    }
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

    /// Run a callback with the LSP client for the focused buffer's language.
    fn with_lsp_client<F>(&mut self, f: F) -> Option<String>
    where
        F: FnOnce(&mut LspClient, &str, novim_types::Position) -> Option<String>,
    {
        let idx = self.active_tab;
        let uri = self.tabs[idx].get_focused_uri()?;
        let cursor = self.tabs[idx].panes.focused_pane().content.as_buffer_like().cursor();
        let display = self.tabs[idx].panes.focused_pane().content.as_buffer_like().display_name();
        let ext = display.rsplit('.').next().unwrap_or("").to_string();
        let server = self.tabs[idx].lsp_registry.resolve(&ext)?;
        let lang_id = server.language_id.clone();
        let client = self.tabs[idx].lsp_clients.get_mut(&lang_id)?;
        f(client, &uri, cursor)
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
                | EditorCommand::DeleteSelection | EditorCommand::Paste
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

    /// Store text in a register. Also updates system clipboard for unnamed/+ register.
    fn yank_to_register(&mut self, text: &str) {
        let reg = self.pending_register.take().unwrap_or('"');
        self.registers.insert(reg, text.to_string());
        // Unnamed register always gets a copy
        if reg != '"' {
            self.registers.insert('"', text.to_string());
        }
        // System clipboard for unnamed or +
        if reg == '"' || reg == '+' {
            set_system_clipboard(text);
        }
    }

    /// Read text from a register. Falls back to system clipboard for unnamed/+.
    fn paste_from_register(&mut self) -> String {
        let reg = self.pending_register.take().unwrap_or('"');
        if reg == '+' || reg == '"' {
            // Prefer system clipboard, fall back to register
            get_system_clipboard()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.registers.get(&reg).cloned().unwrap_or_default())
        } else {
            self.registers.get(&reg).cloned().unwrap_or_default()
        }
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

    /// Build a read-only snapshot of the focused buffer for plugin dispatch.
    fn make_buffer_snapshot(&self) -> crate::plugin::BufferSnapshot {
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
        crate::plugin::BufferSnapshot {
            lines: (0..buf.len_lines()).filter_map(|i| buf.get_line(i)).collect(),
            line_count: buf.len_lines(),
            cursor_line: cursor.line,
            cursor_col: cursor.column,
            path: full_path.or_else(|| Some(buf.display_name())),
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
                    self.plugin_popup = Some(PluginPopup {
                        title, lines, scroll: 0, selected: 0, width, height, on_select,
                    });
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
            }
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

    /// Map a command to the editor events it should emit (called before execution
    /// so we can capture pre-state like file paths).
    fn events_for_command(cmd: &EditorCommand, state: &EditorState) -> Vec<crate::plugin::EditorEvent> {
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
            | EditorCommand::DeleteLines(_)
            | EditorCommand::DeleteMotion(..)
            | EditorCommand::ChangeMotion(..)
            | EditorCommand::ChangeLines(_)
            | EditorCommand::ReplaceAll(..)
            | EditorCommand::Undo
            | EditorCommand::Redo
            | EditorCommand::DeleteSelection
            | EditorCommand::DeleteTextObject(..)
            | EditorCommand::ChangeTextObject(..)
            | EditorCommand::CompletionAccept => {
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

    fn execute_inner(
        &mut self,
        cmd: EditorCommand,
        screen_area: novim_types::Rect,
    ) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        match cmd {
            EditorCommand::Quit => self.handle_quit(),
            EditorCommand::ForceQuit => Ok(ExecOutcome::Quit),
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
            EditorCommand::SwitchMode(mode) => {
                self.focused_buf_mut().break_undo_group();
                if self.mode.is_visual() && !mode.is_visual() {
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
            EditorCommand::SaveSession(name) => self.handle_save_session(&name),
            EditorCommand::EditFile(path) => {
                self.push_jump();
                self.handle_edit_file(&path)
            }
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
                self.mode = EditorMode::Normal;
                self.command_buffer.clear();
                let parsed = parse_ex_command(&cmd_str);
                self.execute(parsed, screen_area)
            }
            EditorCommand::CommandCancel => {
                self.command_buffer.clear();
                self.mode = EditorMode::Normal;
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::MoveCursorN(dir, n) => {
                let buf = self.focused_buf_mut();
                for _ in 0..n {
                    buf.move_cursor(dir);
                }
                Ok(ExecOutcome::Continue)
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
                if let Some(pattern) = &self.search.pattern.clone() {
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_forward(pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    } else {
                        self.status_message = Some("No more matches".to_string());
                    }
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PrevMatch => {
                if let Some(pattern) = &self.search.pattern.clone() {
                    let cursor = self.focused_buf().cursor();
                    if let Some(pos) = self.focused_buf().search_backward(pattern, cursor) {
                        self.focused_buf_mut().set_cursor_pos(pos);
                    } else {
                        self.status_message = Some("No more matches".to_string());
                    }
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
            EditorCommand::ForceRedraw => Ok(ExecOutcome::Continue),
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
            EditorCommand::Echo(msg) => {
                self.status_message = Some(msg);
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
            EditorCommand::DotRepeat => {
                if let Some(edit) = self.last_edit.clone() {
                    let saved = edit.clone();
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
                    self.last_edit = Some(saved);
                } else {
                    self.status_message = Some("No last edit to repeat".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::SelectRegister(c) => {
                self.pending_register = Some(c);
                self.status_message = Some(format!("\"{}",c));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::YankLines(n) => {
                // Yank N lines starting at cursor (without deleting)
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
            EditorCommand::JumpBack => {
                if self.jump_index > 0 {
                    self.jump_index -= 1;
                    if let Some((path, pos)) = self.jump_list.get(self.jump_index).cloned() {
                        let current_name = self.focused_buf().display_name();
                        if path != current_name {
                            let _ = self.handle_edit_file(&path);
                        }
                        self.focused_buf_mut().set_cursor_pos(pos);
                    }
                } else {
                    self.status_message = Some("Already at oldest jump".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::JumpForward => {
                if self.jump_index + 1 < self.jump_list.len() {
                    self.jump_index += 1;
                    if let Some((path, pos)) = self.jump_list.get(self.jump_index).cloned() {
                        let current_name = self.focused_buf().display_name();
                        if path != current_name {
                            let _ = self.handle_edit_file(&path);
                        }
                        self.focused_buf_mut().set_cursor_pos(pos);
                    }
                } else {
                    self.status_message = Some("Already at newest jump".to_string());
                }
                Ok(ExecOutcome::Continue)
            }
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
            EditorCommand::SourceFile(path) => {
                let resolved = if path.starts_with('/') || path.starts_with('~') {
                    let p = path.replace('~', &std::env::var("HOME").unwrap_or_default());
                    std::path::PathBuf::from(p)
                } else {
                    self.tabs[self.active_tab].shell_cwd().join(&path)
                };
                self.plugins.reload_file(&resolved);
                self.status_message = Some(format!("Reloaded: {}", resolved.display()));
                Ok(ExecOutcome::Continue)
            }
            EditorCommand::PluginCommand(name, args) => {
                if self.plugins.has_command(&name) {
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
            EditorCommand::Noop => Ok(ExecOutcome::Continue),
        }
    }

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
                        let border_offset = 1u16;
                        let col_offset = if is_terminal { 1 } else { 6 };

                        let local_y = click_y.saturating_sub(rect.y + border_offset);
                        let local_x = click_x.saturating_sub(rect.x + col_offset);

                        if let Some(pane) = self.tabs[idx].panes.get_pane_mut(*pane_id) {
                            let line = pane.viewport_offset + local_y as usize;
                            let col = local_x as usize;
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

    /// Poll all LSP clients for events in the active workspace.
    pub fn poll_active_lsp(&mut self) {
        let idx = self.active_tab;

        let client_ids: Vec<String> = self.tabs[idx].lsp_clients.keys().cloned().collect();
        for lang_id in client_ids {
            if let Some(client) = self.tabs[idx].lsp_clients.get_mut(&lang_id) {
                let events = client.poll();
                for event in events {
                    match event {
                        LspEvent::Diagnostics { uri, diagnostics } => {
                            self.tabs[idx].diagnostics.insert(uri, diagnostics);
                        }
                        LspEvent::GotoDefinitionResponse { uri, line, col } => {
                            self.tabs[idx].pending_goto = Some((uri, line, col));
                        }
                        LspEvent::HoverResponse { contents } => {
                            self.hover_text = Some(contents);
                        }
                        LspEvent::CompletionResponse { items } => {
                            if !items.is_empty() {
                                self.status_message = Some(format!("{} completions received", items.len()));
                                self.completion.items = items;
                                self.completion.selected = 0;
                                self.completion.visible = true;
                            } else {
                                self.status_message = Some("No completions available".to_string());
                            }
                        }
                        LspEvent::ServerError(msg) => {
                            self.status_message = Some(format!("LSP: {}", msg));
                        }
                        LspEvent::Progress { message } => {
                            self.tabs[idx].lsp_status = Some(message);
                        }
                        LspEvent::ServerExited => {
                            self.tabs[idx].lsp_status = Some("exited".to_string());
                            self.status_message = Some(format!("LSP [{}] exited", lang_id));
                        }
                        LspEvent::Initialized => {
                            self.tabs[idx].lsp_status = Some("Ready".to_string());
                            self.status_message = Some(format!("LSP [{}] ready", lang_id));
                        }
                    }
                }
            }
        }

        // Handle pending goto-definition
        if let Some((uri, line, col)) = self.tabs[idx].pending_goto.take() {
            let path = uri.strip_prefix("file://").unwrap_or(&uri);
            let _ = self.handle_edit_file(path);
            let idx = self.active_tab;
            self.tabs[idx].panes.focused_pane_mut().content.as_buffer_like_mut()
                .set_cursor_pos(novim_types::Position::new(line as usize, col as usize));
            self.status_message = Some("Jumped to definition".to_string());
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

    fn open_explorer_at(&mut self, path: Option<&str>) {
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

    fn handle_quit(&mut self) -> Result<ExecOutcome, NovimError> {
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

    fn handle_save(&mut self) -> Result<ExecOutcome, NovimError> {
        match self.focused_buf_mut().save() {
            Ok(msg) => self.status_message = Some(msg),
            Err(e) => return Err(e),
        }
        Ok(ExecOutcome::Continue)
    }

    fn handle_split(&mut self, direction: SplitDirection) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        self.tabs[idx].panes.split(direction);
        self.status_message = Some(format!("Split ({} panes)", self.tabs[idx].panes.pane_count()));
        Ok(ExecOutcome::Continue)
    }

    fn handle_split_terminal(&mut self, direction: SplitDirection, screen_area: novim_types::Rect) -> Result<ExecOutcome, NovimError> {
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

    fn handle_close_pane(&mut self) -> Result<ExecOutcome, NovimError> {
        let idx = self.active_tab;
        if self.tabs[idx].panes.pane_count() > 1 {
            self.tabs[idx].panes.close_focused();
        } else {
            return Err(NovimError::Command("Cannot close last pane".to_string()));
        }
        Ok(ExecOutcome::Continue)
    }

    fn handle_save_session(&mut self, name: &str) -> Result<ExecOutcome, NovimError> {
        let workspaces: Vec<(String, &PaneManager, String)> = self.tabs
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

    fn handle_open_terminal(&mut self, screen_area: novim_types::Rect) -> Result<ExecOutcome, NovimError> {
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

    fn handle_set_option(&mut self, opt: &str) -> Result<ExecOutcome, NovimError> {
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
