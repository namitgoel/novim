//! Per-workspace state: panes, explorer, LSP, buffer history.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer::Buffer;
use crate::explorer::Explorer;
use crate::lsp::{LspClient, LspEvent};
use crate::lsp::provider::LspRegistry;
use crate::pane::{PaneContent, PaneManager};

use super::types::LspPollResult;

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
    pub(super) lsp_registry: Arc<LspRegistry>,
    pub lsp_clients: HashMap<String, LspClient>,
    pub diagnostics: HashMap<String, Vec<crate::lsp::Diagnostic>>,
    pub(super) pending_goto: Option<(String, u32, u32)>,
    pub lsp_status: Option<String>,
    pub show_buffer_list: bool,
}

impl Workspace {
    pub(super) fn new_with(name: &str, panes: PaneManager, registry: Arc<LspRegistry>) -> Self {
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

    /// Poll all LSP clients and return collected events.
    pub fn poll_lsp_events(&mut self) -> LspPollResult {
        let mut result = LspPollResult::default();
        let client_ids: Vec<String> = self.lsp_clients.keys().cloned().collect();
        for lang_id in client_ids {
            if let Some(client) = self.lsp_clients.get_mut(&lang_id) {
                for event in client.poll() {
                    match event {
                        LspEvent::Diagnostics { uri, diagnostics } => {
                            result.diagnostics.push((uri, diagnostics));
                        }
                        LspEvent::GotoDefinitionResponse { uri, line, col } => {
                            result.goto = Some((uri, line, col));
                        }
                        LspEvent::HoverResponse { contents } => {
                            result.hover = Some(contents);
                        }
                        LspEvent::CompletionResponse { items } => {
                            if !items.is_empty() {
                                result.completions = Some(items);
                            }
                        }
                        LspEvent::ServerError(msg) => {
                            result.status_messages.push(format!("LSP: {}", msg));
                        }
                        LspEvent::Progress { message } => {
                            result.status_messages.push(message);
                        }
                        LspEvent::ServerExited => {
                            result.status_messages.push(format!("LSP [{}] exited", lang_id));
                        }
                        LspEvent::Initialized => {
                            result.status_messages.push(format!("LSP [{}] ready", lang_id));
                        }
                    }
                }
            }
        }
        result
    }

    /// Poll all LSP clients for events (diagnostics, goto responses).
    /// Used for background (non-active) workspaces.
    pub fn poll_lsp(&mut self) {
        let result = self.poll_lsp_events();
        for (uri, diags) in result.diagnostics {
            self.diagnostics.insert(uri, diags);
        }
        if let Some(goto) = result.goto {
            self.pending_goto = Some(goto);
        }
        if let Some(msg) = result.status_messages.last() {
            self.lsp_status = Some(msg.clone());
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
                Err(e) => {
                    log::warn!("Failed to spawn LSP server for {}: {}", server.language_id, e);
                }
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

    pub(super) fn get_focused_uri(&self) -> Option<String> {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.file_uri(),
            _ => None,
        }
    }

    pub(super) fn get_focused_text(&self) -> String {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.full_text(),
            _ => String::new(),
        }
    }

    pub(super) fn get_focused_version(&self) -> i32 {
        match &self.panes.focused_pane().content {
            PaneContent::Editor(buf) => buf.version(),
            _ => 0,
        }
    }
}
