//! Type definitions and small helpers for the editor module.

use crossterm::event::KeyEvent;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::finder;
use crate::highlight;
use crate::input::EditorCommand;
use crate::lsp;

/// Copy text to the system clipboard (best-effort, silent failure).
pub(super) fn set_system_clipboard(text: &str) {
    if let Ok(mut clip) = arboard::Clipboard::new() {
        let _ = clip.set_text(text.to_string());
    }
}

/// Read text from the system clipboard.
pub(super) fn get_system_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Result of executing an editor command.
pub enum ExecOutcome {
    /// Continue the event loop.
    Continue,
    /// Exit the editor.
    Quit,
}

/// Pre-computed status bar content shared between TUI and GUI renderers.
pub struct StatusBarInfo {
    /// Display name for current mode (e.g. "NORMAL", "INSERT", "REC @a", "CTRL+W...", "TERMINAL").
    pub mode_name: String,
    /// Diagnostics summary (e.g. " 2E 1W") or empty string.
    pub diag_summary: String,
    /// LSP status (e.g. " LSP:rust") or empty string.
    pub lsp_status: String,
    /// Pane info (e.g. " [pane 1/3]") or empty string.
    pub pane_info: String,
    /// Git branch (e.g. " main") or empty string.
    pub git_branch: String,
    /// Cursor line (1-based).
    pub cursor_line: usize,
    /// Cursor column (1-based).
    pub cursor_col: usize,
    /// Total lines in the buffer.
    pub total_lines: usize,
    /// Status message if any.
    pub status_message: Option<String>,
    /// Current file name.
    pub file_name: String,
    /// Whether the buffer has unsaved changes.
    pub is_dirty: bool,
}

impl StatusBarInfo {
    /// Format the left side of the status bar using a template string.
    /// Placeholders: {mode}, {message}, {diag}, {pane}, {file}, {modified}
    pub fn format_left(&self, template: &str) -> String {
        let msg = self.status_message.as_deref().unwrap_or("");
        let msg_section = if msg.is_empty() {
            String::new()
        } else {
            msg.to_string()
        };
        template
            .replace("{mode}", &self.mode_name)
            .replace("{message}", &msg_section)
            .replace("{diag}", &self.diag_summary)
            .replace("{pane}", &self.pane_info)
            .replace("{file}", &self.file_name)
            .replace("{modified}", if self.is_dirty { "[+]" } else { "" })
    }

    /// Format the right side of the status bar using a template string.
    /// Placeholders: {lsp}, {branch}, {line}, {col}, {total}, {percent}
    pub fn format_right(&self, template: &str) -> String {
        let pct = if self.total_lines > 0 {
            self.cursor_line * 100 / self.total_lines
        } else { 0 };
        template
            .replace("{lsp}", &self.lsp_status)
            .replace("{branch}", &self.git_branch)
            .replace("{line}", &self.cursor_line.to_string())
            .replace("{col}", &self.cursor_col.to_string())
            .replace("{total}", &self.total_lines.to_string())
            .replace("{percent}", &format!("{}%", pct))
    }
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

/// State for interactive `:s///c` confirm substitution.
#[derive(Default)]
pub struct ConfirmReplaceState {
    /// Whether confirm-replace mode is active.
    pub active: bool,
    /// The search pattern (may include `(?i)` prefix for case-insensitive).
    pub pattern: String,
    /// The original literal pattern (without regex prefix), used for match length.
    pub literal_pattern: String,
    /// The replacement string.
    pub replacement: String,
    /// Current match position (cursor is here).
    pub current_match: Option<novim_types::Position>,
    /// Number of replacements made so far.
    pub replaced_count: usize,
}

/// Record of an edit operation for dot repeat.
#[derive(Debug, Clone)]
pub struct EditRecord {
    /// The initial edit command (e.g. DeleteLines, ChangeMotion, etc.)
    pub command: EditorCommand,
    /// Text typed during insert mode (for change commands).
    pub insert_text: Vec<EditorCommand>,
}

/// Tab completion state for `:` command mode.
#[derive(Default)]
pub struct CommandCompletionState {
    /// Completion candidates for the current word.
    pub candidates: Vec<String>,
    /// Currently selected candidate index.
    pub selected: usize,
    /// The original text before completion started (to restore on cancel).
    pub original: String,
    /// Whether completion is active.
    pub active: bool,
}

/// A single quickfix entry (file:line:col: message).
#[derive(Debug, Clone)]
pub struct QuickfixEntry {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// Quickfix list state.
#[derive(Default)]
pub struct QuickfixState {
    pub entries: Vec<QuickfixEntry>,
    pub current: usize,
    pub visible: bool,
}

/// Command history window state.
#[derive(Default)]
pub struct CommandWindowState {
    pub visible: bool,
    pub selected: usize,
}

/// Symbol list popup state.
pub struct SymbolListState {
    pub visible: bool,
    pub symbols: Vec<crate::highlight::SymbolInfo>,
    pub filtered: Vec<usize>,  // indices into symbols
    pub selected: usize,
    pub query: String,
}

impl Default for SymbolListState {
    fn default() -> Self {
        Self { visible: false, symbols: Vec::new(), filtered: Vec::new(), selected: 0, query: String::new() }
    }
}

impl SymbolListState {
    pub fn filter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self.symbols.iter().enumerate()
            .filter(|(_, s)| q.is_empty() || s.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }
}

/// Symbol outline sidebar state.
pub struct OutlineState {
    pub visible: bool,
    pub symbols: Vec<crate::highlight::SymbolInfo>,
    pub selected: usize,
    /// Cached breadcrumb string for the status/header area.
    pub breadcrumb: String,
}

impl Default for OutlineState {
    fn default() -> Self {
        Self { visible: false, symbols: Vec::new(), selected: 0, breadcrumb: String::new() }
    }
}

/// A floating window displayed on top of the editor.
pub struct FloatingWindow {
    pub title: String,
    pub lines: Vec<String>,
    pub width: u16,
    pub height: u16,
    pub scroll: usize,
    pub selected: usize,
}

impl CommandCompletionState {
    pub fn reset(&mut self) {
        self.candidates.clear();
        self.selected = 0;
        self.original.clear();
        self.active = false;
    }
}

pub(super) fn ln_mode_from_config(s: &str) -> LineNumberMode {
    match s {
        "absolute" | "number" => LineNumberMode::Absolute,
        "relative" => LineNumberMode::Relative,
        "hybrid" => LineNumberMode::Hybrid,
        "off" | "none" => LineNumberMode::Off,
        _ => LineNumberMode::Hybrid,
    }
}
