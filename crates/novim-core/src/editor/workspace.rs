//! Per-workspace state: panes, explorer, buffer history, diagnostics.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer::Buffer;
use crate::explorer::Explorer;
use crate::lsp::provider::LspRegistry;
use crate::pane::PaneManager;

/// Per-workspace state: panes, explorer, buffer history, diagnostics.
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
    /// Diagnostics from LSP plugin (per-file URI).
    pub diagnostics: HashMap<String, Vec<crate::lsp::Diagnostic>>,
    /// LSP status message (e.g. "LSP:rust").
    pub lsp_status: Option<String>,
    pub show_buffer_list: bool,
}

impl Workspace {
    pub(super) fn new_with(name: &str, panes: PaneManager, _registry: Arc<LspRegistry>) -> Self {
        Self {
            name: name.to_string(),
            panes,
            explorer: None,
            explorer_focused: false,
            buffer_history: Vec::new(),
            buffer_history_idx: 0,
            launch_dir: std::env::current_dir().unwrap_or_default(),
            last_shell_cwd: None,
            diagnostics: HashMap::new(),
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

}
