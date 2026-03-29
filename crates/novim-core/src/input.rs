//! Input handling with the Command pattern

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use novim_types::{Direction, EditorMode};

use crate::pane::SplitDirection;

/// Convert a key event to a string representation for config lookup.
/// Examples: "u", "Ctrl+s", "Ctrl+r", "Esc", "Enter"
fn key_to_string(key: &KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }

    let key_name = match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        _ => return String::new(),
    };
    parts.push(key_name);
    parts.join("+")
}

/// Parse a command name string from config into an EditorCommand.
pub fn command_from_name(name: &str) -> EditorCommand {
    match name {
        "quit" => EditorCommand::Quit,
        "force_quit" => EditorCommand::ForceQuit,
        "save" => EditorCommand::Save,
        "save_and_quit" => EditorCommand::SaveAndQuit,
        "undo" => EditorCommand::Undo,
        "redo" => EditorCommand::Redo,
        "insert_mode" => EditorCommand::SwitchMode(EditorMode::Insert),
        "normal_mode" => EditorCommand::SwitchMode(EditorMode::Normal),
        "command_mode" => EditorCommand::SwitchMode(EditorMode::Command),
        "visual_mode" => EditorCommand::EnterVisual,
        "search" => EditorCommand::EnterSearch,
        "next_match" => EditorCommand::NextMatch,
        "prev_match" => EditorCommand::PrevMatch,
        "clear_search" => EditorCommand::ClearSearch,
        "paste" => EditorCommand::Paste,
        "move_left" => EditorCommand::MoveCursor(Direction::Left),
        "move_down" => EditorCommand::MoveCursor(Direction::Down),
        "move_up" => EditorCommand::MoveCursor(Direction::Up),
        "move_right" => EditorCommand::MoveCursor(Direction::Right),
        "scroll_up" => EditorCommand::ScrollUp,
        "scroll_down" => EditorCommand::ScrollDown,
        "goto_definition" => EditorCommand::GotoDefinition,
        "toggle_help" => EditorCommand::ToggleHelp,
        "toggle_explorer" => EditorCommand::ToggleExplorer,
        "split_vertical" => EditorCommand::SplitPane(SplitDirection::Vertical),
        "split_horizontal" => EditorCommand::SplitPane(SplitDirection::Horizontal),
        "open_terminal" => EditorCommand::OpenTerminal,
        "buffer_next" => EditorCommand::BufferNext,
        "buffer_prev" => EditorCommand::BufferPrev,
        "buffer_list" => EditorCommand::BufferList,
        _ => EditorCommand::Noop,
    }
}

/// Try to resolve a key event from custom keybindings.
/// Returns Some(command) if found, None to fall back to defaults.
pub fn lookup_custom_keybinding(
    _mode: EditorMode,
    key: &KeyEvent,
    bindings: &HashMap<String, String>,
) -> Option<EditorCommand> {
    let key_str = key_to_string(key);
    if key_str.is_empty() {
        return None;
    }
    bindings.get(&key_str).map(|cmd_name| command_from_name(cmd_name))
}

/// A discrete editor action.
pub enum EditorCommand {
    Quit,
    /// Force quit (ignore unsaved changes)
    ForceQuit,
    MoveCursor(Direction),
    SwitchMode(EditorMode),
    InsertChar(char),
    InsertTab,
    AddCursorAbove,
    AddCursorBelow,
    ClearSecondaryCursors,
    ToggleFold,
    FoldAll,
    UnfoldAll,
    DeleteCharBefore,
    InsertNewline,
    Save,
    /// Save and quit
    SaveAndQuit,
    SplitPane(SplitDirection),
    FocusDirection(Direction),
    FocusNext,
    ClosePane,
    OpenTerminal,
    ForwardToTerminal(KeyEvent),
    SaveSession(String),
    /// Open a file in the current pane
    EditFile(String),
    /// Append to the command buffer
    CommandInput(char),
    /// Delete last char in command buffer
    CommandBackspace,
    /// Execute the command buffer
    CommandExecute,
    /// Cancel command mode
    CommandCancel,
    /// Force full screen redraw (Ctrl+L)
    ForceRedraw,
    Undo,
    Redo,
    /// Set an editor option (e.g., :set rnu)
    SetOption(String),
    /// Enter visual mode (start selection at cursor)
    EnterVisual,
    /// Delete selection and return to Normal
    DeleteSelection,
    /// Yank (copy) selection and return to Normal
    YankSelection,
    /// Paste from clipboard
    Paste,
    /// Switch to next buffer
    BufferNext,
    /// Switch to previous buffer
    BufferPrev,
    /// List open buffers
    BufferList,
    /// Scroll viewport up (half page)
    ScrollUp,
    /// Scroll viewport down (half page)
    ScrollDown,
    /// Enter search mode (/ prefix)
    EnterSearch,
    /// Append to search buffer
    SearchInput(char),
    /// Delete last char in search buffer
    SearchBackspace,
    /// Execute search
    SearchExecute,
    /// Cancel search
    SearchCancel,
    /// Jump to next match
    NextMatch,
    /// Jump to previous match
    PrevMatch,
    /// Replace all: (pattern, replacement)
    ReplaceAll(String, String),
    /// Clear search highlights
    ClearSearch,
    /// Start recording macro into a register
    StartMacroRecord(char),
    /// Stop recording macro
    StopMacroRecord,
    /// Replay macro from a register
    ReplayMacro(char),
    /// Trigger autocomplete
    TriggerCompletion,
    /// Navigate completion menu up
    CompletionUp,
    /// Navigate completion menu down
    CompletionDown,
    /// Accept selected completion
    CompletionAccept,
    /// Dismiss completion menu
    CompletionDismiss,
    /// Toggle file explorer sidebar (current dir)
    ToggleExplorer,
    /// Open explorer at a specific path
    OpenExplorer(String),
    /// Explorer navigation
    ExplorerUp,
    ExplorerDown,
    ExplorerOpen,
    /// Move cursor N times
    MoveCursorN(Direction, usize),
    /// Delete from cursor in direction N times (d + motion)
    DeleteMotion(Direction, usize),
    /// Change from cursor in direction N times (c + motion) — delete + enter insert
    ChangeMotion(Direction, usize),
    /// Delete N whole lines (dd)
    DeleteLines(usize),
    /// Change N whole lines (cc)
    ChangeLines(usize),
    /// Go to definition at cursor (LSP)
    GotoDefinition,
    /// Focus the explorer sidebar
    FocusExplorer,
    /// Show hover info at cursor (LSP)
    ShowHover,
    /// Open file finder (Ctrl+P) in current directory
    OpenFileFinder,
    /// Open file finder at a specific path
    OpenFinderAt(String),
    /// File finder input
    FinderInput(char),
    FinderBackspace,
    FinderUp,
    FinderDown,
    FinderAccept,
    FinderDismiss,
    /// Open a new tab with a directory
    OpenTab(String),
    /// Switch to next/prev tab
    NextTab,
    PrevTab,
    /// Close current tab/workspace
    CloseTab,
    /// Toggle workspace list popup
    ListWorkspaces,
    /// Jump to a specific tab by index
    JumpToTab(usize),
    /// Rename the current tab/workspace
    RenameTab(String),
    /// Toggle help popup (shortcuts)
    ToggleHelp,
    /// Dismiss popup (Esc/q when popup is showing)
    DismissPopup,
    Noop,
}

/// Tracks multi-key input states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputState {
    Ready,
    WaitingPaneCommand,
    WaitingGCommand,
    /// Waiting for fold sub-command after 'z'
    WaitingZCommand,
    /// Accumulating a count prefix
    AccumulatingCount,
    /// Waiting for motion after operator (d, c)
    WaitingOperatorMotion(Operator),
    /// Waiting for register name after 'q' (start recording)
    WaitingMacroRegister,
    /// Waiting for register name after '@' (replay)
    WaitingReplayRegister,
}

/// Pending operator (d = delete, c = change)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
}

/// Mutable state for count + operator tracking.
/// Must be stored alongside InputState in EditorState.
#[derive(Debug, Clone, Default)]
pub struct CountState {
    pub count: Option<usize>,
    pub pending_digits: String,
}

/// Map a key to a command.
pub fn key_to_command(
    mode: EditorMode,
    input_state: InputState,
    key: KeyEvent,
    in_terminal: bool,
    popup_showing: bool,
) -> (EditorCommand, InputState) {
    // When a popup is showing, Esc/q/? dismiss it, everything else is ignored
    if popup_showing {
        return match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                (EditorCommand::DismissPopup, InputState::Ready)
            }
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Ctrl+w always enters pane command mode
    if input_state == InputState::Ready
        && key.code == KeyCode::Char('w')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return (EditorCommand::Noop, InputState::WaitingPaneCommand);
    }

    if input_state == InputState::WaitingPaneCommand {
        return (pane_command(key), InputState::Ready);
    }

    if input_state == InputState::WaitingGCommand {
        return match key.code {
            KeyCode::Char('d') => (EditorCommand::GotoDefinition, InputState::Ready),
            KeyCode::Char('t') => (EditorCommand::NextTab, InputState::Ready),
            KeyCode::Char('T') => (EditorCommand::PrevTab, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if input_state == InputState::WaitingZCommand {
        return match key.code {
            KeyCode::Char('a') => (EditorCommand::ToggleFold, InputState::Ready),
            KeyCode::Char('M') => (EditorCommand::FoldAll, InputState::Ready),
            KeyCode::Char('R') => (EditorCommand::UnfoldAll, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return (EditorCommand::ForceQuit, InputState::Ready);
    }

    // Ctrl+L forces full redraw
    if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return (EditorCommand::ForceRedraw, InputState::Ready);
    }

    // Command and search modes take priority (even in terminal panes)
    if mode == EditorMode::Command {
        return (command_mode_command(key), InputState::Ready);
    }

    // Terminal panes: forward all other keys to PTY
    if in_terminal {
        return (EditorCommand::ForwardToTerminal(key), InputState::Ready);
    }

    // Handle count accumulation (digits in Normal mode)
    if mode == EditorMode::Normal && input_state == InputState::AccumulatingCount {
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() {
                return (EditorCommand::Noop, InputState::AccumulatingCount); // keep accumulating
            }
        }
        // Non-digit: apply the count to the next command
        // Count is handled by EditorState which tracks CountState
    }

    // Handle operator-pending mode (d/c + motion)
    if let InputState::WaitingOperatorMotion(op) = input_state {
        return (operator_motion_command(key, op), InputState::Ready);
    }

    // Handle macro register selection
    if input_state == InputState::WaitingMacroRegister {
        return match key.code {
            KeyCode::Char(c) if c.is_ascii_lowercase() => {
                (EditorCommand::StartMacroRecord(c), InputState::Ready)
            }
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if input_state == InputState::WaitingReplayRegister {
        return match key.code {
            KeyCode::Char(c) if c.is_ascii_lowercase() => {
                (EditorCommand::ReplayMacro(c), InputState::Ready)
            }
            KeyCode::Char('@') => {
                // @@ = replay last macro (use special char)
                (EditorCommand::ReplayMacro('@'), InputState::Ready)
            }
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    match mode {
        EditorMode::Normal => normal_mode_command(key),
        EditorMode::Insert => (insert_mode_command(key), InputState::Ready),
        EditorMode::Visual => (visual_mode_command(key), InputState::Ready),
        _ => (EditorCommand::Noop, InputState::Ready),
    }
}

fn pane_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Char('s') => EditorCommand::SplitPane(SplitDirection::Horizontal),
        KeyCode::Char('v') => EditorCommand::SplitPane(SplitDirection::Vertical),
        KeyCode::Char('h') | KeyCode::Left => EditorCommand::FocusDirection(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => EditorCommand::FocusDirection(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => EditorCommand::FocusDirection(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => EditorCommand::FocusDirection(Direction::Right),
        KeyCode::Char('w') => EditorCommand::FocusNext,
        KeyCode::Char('q') => EditorCommand::ClosePane,
        KeyCode::Char('t') => EditorCommand::OpenTerminal,
        KeyCode::Char('f') | KeyCode::Char('p') => EditorCommand::OpenFileFinder,
        KeyCode::Char('e') => EditorCommand::FocusExplorer,
        KeyCode::Char('L') => EditorCommand::ListWorkspaces,
        // Workspace switching (also works from terminal panes)
        KeyCode::Char('n') => EditorCommand::NextTab,
        KeyCode::Char('N') => EditorCommand::PrevTab,
        KeyCode::Char('1') => EditorCommand::JumpToTab(0),
        KeyCode::Char('2') => EditorCommand::JumpToTab(1),
        KeyCode::Char('3') => EditorCommand::JumpToTab(2),
        KeyCode::Char('4') => EditorCommand::JumpToTab(3),
        KeyCode::Char('5') => EditorCommand::JumpToTab(4),
        KeyCode::Char('6') => EditorCommand::JumpToTab(5),
        KeyCode::Char('7') => EditorCommand::JumpToTab(6),
        KeyCode::Char('8') => EditorCommand::JumpToTab(7),
        KeyCode::Char('9') => EditorCommand::JumpToTab(8),
        KeyCode::Char('?') => EditorCommand::ToggleHelp,
        KeyCode::Char(':') => EditorCommand::SwitchMode(EditorMode::Command),
        _ => EditorCommand::Noop,
    }
}

fn normal_mode_command(key: KeyEvent) -> (EditorCommand, InputState) {
    match key.code {
        // Enter command mode
        KeyCode::Char(':') => (
            EditorCommand::SwitchMode(EditorMode::Command),
            InputState::Ready,
        ),
        KeyCode::Char('?') => (EditorCommand::ToggleHelp, InputState::Ready),
        // Scroll: Ctrl+U up half page, Ctrl+D down half page
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::OpenFileFinder, InputState::Ready)
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::OpenFileFinder, InputState::Ready)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::ScrollUp, InputState::Ready)
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::ScrollDown, InputState::Ready)
        }
        // Multi-cursor: Alt+Up/Down to add cursors
        KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
            (EditorCommand::AddCursorAbove, InputState::Ready)
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
            (EditorCommand::AddCursorBelow, InputState::Ready)
        }
        KeyCode::Char('u') => (EditorCommand::Undo, InputState::Ready),
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Redo, InputState::Ready)
        }
        KeyCode::Char('/') => (EditorCommand::EnterSearch, InputState::Ready),
        KeyCode::Char('n') => (EditorCommand::NextMatch, InputState::Ready),
        KeyCode::Char('N') => (EditorCommand::PrevMatch, InputState::Ready),
        KeyCode::Char('K') => (EditorCommand::ShowHover, InputState::Ready),
        KeyCode::Char('g') => (EditorCommand::Noop, InputState::WaitingGCommand),
        KeyCode::Char('z') => (EditorCommand::Noop, InputState::WaitingZCommand),
        KeyCode::Char('v') => (EditorCommand::EnterVisual, InputState::Ready),
        KeyCode::Char('p') => (EditorCommand::Paste, InputState::Ready),
        KeyCode::Esc => (EditorCommand::ClearSearch, InputState::Ready),
        // Count prefix: digits 1-9 start a count, 0 is not a count starter
        KeyCode::Char(_c @ '1'..='9') => (EditorCommand::Noop, InputState::AccumulatingCount),
        // Operators: d = delete, c = change
        KeyCode::Char('d') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Delete)),
        KeyCode::Char('c') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Change)),
        // q: if recording → stop, else → quit (use :q for quit to avoid conflict)
        KeyCode::Char('q') => (EditorCommand::Quit, InputState::Ready),
        // Macro: Q starts recording (shifted to avoid quit conflict)
        KeyCode::Char('Q') => (EditorCommand::Noop, InputState::WaitingMacroRegister),
        KeyCode::Char('@') => (EditorCommand::Noop, InputState::WaitingReplayRegister),
        KeyCode::Char('i') => (
            EditorCommand::SwitchMode(EditorMode::Insert),
            InputState::Ready,
        ),
        KeyCode::Char('h') | KeyCode::Left => {
            (EditorCommand::MoveCursor(Direction::Left), InputState::Ready)
        }
        KeyCode::Char('j') | KeyCode::Down => {
            (EditorCommand::MoveCursor(Direction::Down), InputState::Ready)
        }
        KeyCode::Char('k') | KeyCode::Up => {
            (EditorCommand::MoveCursor(Direction::Up), InputState::Ready)
        }
        KeyCode::Char('l') | KeyCode::Right => (
            EditorCommand::MoveCursor(Direction::Right),
            InputState::Ready,
        ),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Save, InputState::Ready)
        }
        _ => (EditorCommand::Noop, InputState::Ready),
    }
}

fn insert_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc => EditorCommand::SwitchMode(EditorMode::Normal),
        // Ctrl+Space or Ctrl+N triggers autocomplete
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            EditorCommand::TriggerCompletion
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            EditorCommand::TriggerCompletion
        }
        KeyCode::Char(c) => EditorCommand::InsertChar(c),
        KeyCode::Tab => EditorCommand::InsertTab,
        KeyCode::Backspace => EditorCommand::DeleteCharBefore,
        KeyCode::Enter => EditorCommand::InsertNewline,
        KeyCode::Left => EditorCommand::MoveCursor(Direction::Left),
        KeyCode::Right => EditorCommand::MoveCursor(Direction::Right),
        KeyCode::Up => EditorCommand::MoveCursor(Direction::Up),
        KeyCode::Down => EditorCommand::MoveCursor(Direction::Down),
        _ => EditorCommand::Noop,
    }
}

/// Handle the motion key after an operator (d/c).
fn operator_motion_command(key: KeyEvent, op: Operator) -> EditorCommand {
    match key.code {
        // d/c + motion
        KeyCode::Char('h') | KeyCode::Left => match op {
            Operator::Delete => EditorCommand::DeleteMotion(Direction::Left, 1),
            Operator::Change => EditorCommand::ChangeMotion(Direction::Left, 1),
        },
        KeyCode::Char('j') | KeyCode::Down => match op {
            Operator::Delete => EditorCommand::DeleteMotion(Direction::Down, 1),
            Operator::Change => EditorCommand::ChangeMotion(Direction::Down, 1),
        },
        KeyCode::Char('k') | KeyCode::Up => match op {
            Operator::Delete => EditorCommand::DeleteMotion(Direction::Up, 1),
            Operator::Change => EditorCommand::ChangeMotion(Direction::Up, 1),
        },
        KeyCode::Char('l') | KeyCode::Right => match op {
            Operator::Delete => EditorCommand::DeleteMotion(Direction::Right, 1),
            Operator::Change => EditorCommand::ChangeMotion(Direction::Right, 1),
        },
        // dd = delete line, cc = change line
        KeyCode::Char('d') if op == Operator::Delete => EditorCommand::DeleteLines(1),
        KeyCode::Char('c') if op == Operator::Change => EditorCommand::ChangeLines(1),
        // Anything else cancels
        _ => EditorCommand::Noop,
    }
}

fn visual_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc | KeyCode::Char('v') => EditorCommand::SwitchMode(EditorMode::Normal),
        // Movement extends selection
        KeyCode::Char('h') | KeyCode::Left => EditorCommand::MoveCursor(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => EditorCommand::MoveCursor(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => EditorCommand::MoveCursor(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => EditorCommand::MoveCursor(Direction::Right),
        // Actions on selection
        KeyCode::Char('d') | KeyCode::Char('x') => EditorCommand::DeleteSelection,
        KeyCode::Char('y') => EditorCommand::YankSelection,
        _ => EditorCommand::Noop,
    }
}

fn command_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc => EditorCommand::CommandCancel,
        KeyCode::Enter => EditorCommand::CommandExecute,
        KeyCode::Backspace => EditorCommand::CommandBackspace,
        KeyCode::Char(c) => EditorCommand::CommandInput(c),
        _ => EditorCommand::Noop,
    }
}

/// Parse and convert an ex-command string into an EditorCommand.
pub fn parse_ex_command(input: &str) -> EditorCommand {
    let input = input.trim();
    let (cmd, args) = match input.split_once(' ') {
        Some((c, a)) => (c, a.trim()),
        None => (input, ""),
    };

    match cmd {
        "w" => EditorCommand::Save,
        "q" => EditorCommand::Quit,
        "q!" => EditorCommand::ForceQuit,
        "wq" | "x" => EditorCommand::SaveAndQuit,
        "split" | "sp" => EditorCommand::SplitPane(SplitDirection::Horizontal),
        "vsplit" | "vs" => EditorCommand::SplitPane(SplitDirection::Vertical),
        "terminal" | "term" => EditorCommand::OpenTerminal,
        "e" | "edit" => {
            if args.is_empty() {
                EditorCommand::Noop
            } else {
                EditorCommand::EditFile(args.to_string())
            }
        }
        "mksession" => {
            let name = if args.is_empty() { "default" } else { args };
            EditorCommand::SaveSession(name.to_string())
        }
        "close" => EditorCommand::ClosePane,
        "bnext" | "bn" => EditorCommand::BufferNext,
        "bprev" | "bp" => EditorCommand::BufferPrev,
        "ls" | "buffers" => EditorCommand::BufferList,
        "find" | "Files" => {
            if args.is_empty() {
                EditorCommand::OpenFileFinder
            } else {
                EditorCommand::OpenFinderAt(args.to_string())
            }
        }
        "tabnew" | "tabe" => {
            let dir = if args.is_empty() { "." } else { args };
            EditorCommand::OpenTab(dir.to_string())
        }
        "tabn" | "tabnext" => EditorCommand::NextTab,
        "tabp" | "tabprev" => EditorCommand::PrevTab,
        "tabclose" | "tabc" => EditorCommand::CloseTab,
        "tabs" | "workspaces" => EditorCommand::ListWorkspaces,
        "tabrename" => {
            if args.is_empty() {
                EditorCommand::Noop
            } else {
                EditorCommand::RenameTab(args.to_string())
            }
        }
        "noh" | "nohlsearch" => EditorCommand::ClearSearch,
        "definition" | "def" => EditorCommand::GotoDefinition,
        "explore" | "Explore" | "Ex" => {
            if args.is_empty() {
                EditorCommand::ToggleExplorer
            } else {
                EditorCommand::OpenExplorer(args.to_string())
            }
        }
        "undo" => EditorCommand::Undo,
        "redo" => EditorCommand::Redo,
        "help" | "h" => EditorCommand::ToggleHelp,
        "set" => {
            if args.is_empty() {
                EditorCommand::Noop
            } else {
                EditorCommand::SetOption(args.to_string())
            }
        }
        _ => {
            // Handle %s/pattern/replacement/g
            let s = input.strip_prefix('%').unwrap_or(input);
            if let Some(rest) = s.strip_prefix("s/") {
                let parts: Vec<&str> = rest.splitn(3, '/').collect();
                if parts.len() >= 2 {
                    let pattern = parts[0].to_string();
                    let replacement = parts[1].to_string();
                    return EditorCommand::ReplaceAll(pattern, replacement);
                }
            }
            EditorCommand::Noop
        }
    }
}
