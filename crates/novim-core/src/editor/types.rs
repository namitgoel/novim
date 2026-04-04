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

/// Collected LSP events from polling, before applying to state.
#[derive(Default)]
pub struct LspPollResult {
    pub diagnostics: Vec<(String, Vec<lsp::Diagnostic>)>,
    pub goto: Option<(String, u32, u32)>,
    pub hover: Option<String>,
    pub completions: Option<Vec<lsp::CompletionItem>>,
    pub status_messages: Vec<String>,
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

pub(super) fn ln_mode_from_config(s: &str) -> LineNumberMode {
    match s {
        "absolute" | "number" => LineNumberMode::Absolute,
        "relative" => LineNumberMode::Relative,
        "hybrid" => LineNumberMode::Hybrid,
        "off" | "none" => LineNumberMode::Off,
        _ => LineNumberMode::Hybrid,
    }
}
