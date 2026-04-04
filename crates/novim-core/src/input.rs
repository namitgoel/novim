//! Input handling with the Command pattern

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use novim_types::{Direction, EditorMode};

use crate::pane::SplitDirection;

/// Convert a key event to a string representation for config lookup.
/// Examples: "u", "Ctrl+s", "Ctrl+r", "Esc", "Enter"
pub fn key_to_string(key: &KeyEvent) -> String {
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
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
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
#[derive(Debug, Clone)]
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
    /// Replace all: (pattern, replacement, case_insensitive)
    ReplaceAll(String, String, bool),
    /// Interactive confirm substitution: (pattern, replacement, case_insensitive)
    ReplaceConfirm(String, String, bool),
    /// Confirm replace: accept current match
    ReplaceConfirmYes,
    /// Confirm replace: skip current match
    ReplaceConfirmNo,
    /// Confirm replace: replace all remaining
    ReplaceConfirmAll,
    /// Confirm replace: stop
    ReplaceConfirmQuit,
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
    /// Delete a text object (e.g., diw, di", da()
    DeleteTextObject(TextObjectModifier, TextObjectKind),
    /// Change a text object (e.g., ciw, ci", ca()
    ChangeTextObject(TextObjectModifier, TextObjectKind),
    /// Yank a text object without deleting (e.g., yiw, yi")
    YankTextObject(TextObjectModifier, TextObjectKind),
    /// Display a message in the status bar.
    Echo(String),
    /// List all loaded plugins.
    PluginList,
    /// Jump backward in jump list.
    JumpBack,
    /// Jump forward in jump list.
    JumpForward,
    /// Set a mark at the current cursor position.
    SetMark(char),
    /// Jump to a mark. Bool = exact position (true) vs line-only (false).
    JumpToMark(char, bool),
    /// Enter block visual mode (column selection).
    EnterVisualBlock,
    /// Dot repeat — replay last edit.
    DotRepeat,
    /// Select a named register for the next yank/delete/paste.
    SelectRegister(char),
    /// Yank N lines (yy).
    YankLines(usize),
    /// Yank from cursor in direction (y + motion).
    YankMotion(Direction, usize),
    /// Reload a Lua plugin file.
    SourceFile(String),
    /// A plugin-registered command (name, args).
    PluginCommand(String, String),

    // Character find motions
    /// f/F — (char, forward). forward=true for f, false for F
    FindChar(char, bool),
    /// t/T — (char, forward). Like FindChar but stops one before
    TillChar(char, bool),
    /// ; — repeat last f/t/F/T
    RepeatFindChar,
    /// , — repeat in opposite direction
    RepeatFindCharReverse,

    // Single-char operations
    /// r + char — replace char at cursor
    ReplaceChar(char),
    /// x — delete char under cursor
    DeleteCharForward,

    // Line operations
    /// o — new line below, enter insert
    OpenLineBelow,
    /// O — new line above, enter insert
    OpenLineAbove,
    /// A — move to end of line, enter insert
    AppendEndOfLine,
    /// I — move to first non-blank, enter insert
    InsertStartOfLine,
    /// C — delete to end of line, enter insert
    ChangeToEnd,
    /// D — delete to end of line
    DeleteToEnd,
    /// S — delete line content, enter insert
    SubstituteLine,
    /// J — join N lines
    JoinLines(usize),

    // Indentation
    /// >> — indent N lines
    Indent(usize),
    /// << — dedent N lines
    Dedent(usize),

    // Case
    /// ~ — toggle case at cursor
    ToggleCase,

    // Search
    /// * — search word under cursor forward
    SearchWordForward,
    /// # — search word under cursor backward
    SearchWordBackward,

    // Navigation
    /// % — jump to matching bracket
    MatchBracket,

    // Pane resize / zoom
    /// Ctrl+W + — grow pane height
    ResizePaneGrow,
    /// Ctrl+W - — shrink pane height
    ResizePaneShrink,
    /// Ctrl+W > — grow pane width
    ResizePaneWider,
    /// Ctrl+W < — shrink pane width
    ResizePaneNarrower,
    /// Ctrl+W z — toggle zoom on focused pane
    ZoomPane,

    // Command history
    /// Up arrow in command mode — recall previous command
    CommandHistoryUp,
    /// Down arrow in command mode — recall next command
    CommandHistoryDown,
    /// Tab in command mode — complete current word
    CommandTab,
    /// Shift+Tab in command mode — complete backward
    CommandTabBack,

    // Shell execution
    /// :! — run a shell command
    ShellCommand(String),

    // URL / file under cursor
    /// gx — open URL under cursor
    OpenUrlUnderCursor,
    /// gf — open file under cursor
    OpenFileUnderCursor,

    /// P — paste before cursor
    PasteBefore,
    /// zz — scroll so cursor is at center of screen
    ScrollCenter,
    /// zt — scroll so cursor is at top of screen
    ScrollTop,
    /// zb — scroll so cursor is at bottom of screen
    ScrollBottom,
    /// :marks — list all marks
    ListMarks,
    /// :registers / :reg — list all registers
    ListRegisters,
    /// gv — reselect last visual selection
    ReselectVisual,
    /// Ctrl+W x — swap focused pane with next
    SwapPane,

    /// Scroll viewport down by full page
    PageDown,
    /// Scroll viewport up by full page
    PageUp,

    /// Enter terminal copy mode (Ctrl+W [)
    EnterCopyMode,
    /// Exit terminal copy mode
    ExitCopyMode,
    /// Replace mode insert char (overwrites instead of inserting)
    ReplaceInsertChar(char),

    /// gj — move down one display/screen line
    DisplayLineDown,
    /// gk — move up one display/screen line
    DisplayLineUp,

    /// Y — yank to end of line (like y$)
    YankToEnd,
    /// Ctrl+E — scroll viewport down one line (cursor stays)
    ScrollLineDown,
    /// Ctrl+Y — scroll viewport up one line (cursor stays)
    ScrollLineUp,
    /// H — move cursor to top of visible screen
    ScreenTop,
    /// M — move cursor to middle of visible screen
    ScreenMiddle,
    /// L — move cursor to bottom of visible screen
    ScreenBottom,
    /// '' — jump to position before last jump
    JumpToLastPosition,
    /// :cd / :lcd — change working directory
    ChangeDirectory(String),
    /// Ctrl+G — show file info in status bar
    FileInfo,
    /// Indent selected lines in visual mode
    VisualIndent,
    /// Dedent selected lines in visual mode
    VisualDedent,
    /// Toggle case on visual selection
    VisualToggleCase,
    /// Uppercase visual selection
    VisualUpperCase,
    /// Lowercase visual selection
    VisualLowerCase,

    /// Ctrl+A — increment number at/after cursor
    IncrementNumber,
    /// Ctrl+X — decrement number at/after cursor
    DecrementNumber,
    /// gU + motion — uppercase with motion
    UpperCaseMotion(Direction, usize),
    /// gu + motion — lowercase with motion
    LowerCaseMotion(Direction, usize),
    /// gUU / guu — uppercase/lowercase whole line
    UpperCaseLine,
    LowerCaseLine,
    /// :read — insert file or command output below cursor
    ReadFile(String),
    ReadCommand(String),
    /// :sort — sort lines
    SortLines,
    /// :w !cmd — pipe buffer to shell command
    PipeToCommand(String),
    /// == — auto-indent current line
    AutoIndent,

    /// gq + motion — format/wrap text
    FormatMotion(Direction, usize),
    /// gqq — format current line
    FormatLine,
    /// :copen — open quickfix list
    QuickfixOpen,
    /// :cnext / :cn — jump to next quickfix entry
    QuickfixNext,
    /// :cprev / :cp — jump to previous quickfix entry
    QuickfixPrev,
    /// :cclose / :ccl — close quickfix list
    QuickfixClose,
    /// :make — run build command and populate quickfix
    Make(String),
    /// q: — open command history window
    OpenCommandWindow,
    /// :symbols — list symbols in current file
    SymbolList,
    /// Navigate symbol list
    SymbolUp,
    SymbolDown,
    SymbolAccept,
    SymbolDismiss,
    SymbolInput(char),
    SymbolBackspace,
    /// Open/close a floating window (plugin API)
    OpenFloat { title: String, lines: Vec<String>, width: u16, height: u16 },
    CloseFloat,
    /// :blame / :Gblame — toggle inline git blame
    ToggleBlame,
    /// :diff / :Gdiff — open side-by-side diff vs HEAD
    GitDiff,
    /// :PlugInstall <url> — install a plugin from a git URL
    PluginInstall(String),
    /// :PlugUpdate [name] — update installed plugins (or a specific one)
    PluginUpdate(String),
    /// :PlugRemove <name> — remove an installed plugin
    PluginRemove(String),
    /// Toggle minimap
    ToggleMinimap,
    /// Toggle symbol outline sidebar
    ToggleOutline,

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
    /// Waiting for text object kind after operator + i/a (e.g., d→i→?)
    WaitingTextObject(Operator, TextObjectModifier),
    /// Waiting for register name after 'q' (start recording)
    WaitingMacroRegister,
    /// Waiting for register name after '@' (replay)
    WaitingReplayRegister,
    /// Waiting for mark register after 'm'
    WaitingMarkSet,
    /// Waiting for mark register after ' or `. Bool = exact position.
    WaitingMarkJump(bool),
    /// Waiting for register name after '"'
    WaitingRegister,
    /// Waiting for target character after f/F/t/T
    /// (forward, inclusive) — f=(true,true), F=(false,true), t=(true,false), T=(false,false)
    WaitingFindChar(bool, bool),
    /// Waiting for replacement character after r
    WaitingReplaceChar,
    /// Waiting for second > or < (for >> or <<)
    /// true = indent (>), false = dedent (<)
    WaitingIndent(bool),
    /// Waiting for second = (for ==)
    WaitingAutoIndent,
}

/// Pending operator (d = delete, c = change, y = yank, gU = uppercase, gu = lowercase)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    UpperCase,
    LowerCase,
    Format,
}

/// Text object modifier: inner (i) vs around (a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjectModifier {
    Inner,
    Around,
}

/// Text object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjectKind {
    Word,
    Bracket(char, char), // (open, close)
    Quote(char),
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
    gui_mode: bool,
    is_macro_recording: bool,
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

    // Ctrl+w enters pane command mode — but in GUI terminal panes,
    // Cmd+W handles this, so let Ctrl+W flow to the PTY.
    if input_state == InputState::Ready
        && key.code == KeyCode::Char('w')
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !(gui_mode && in_terminal)
    {
        return (EditorCommand::Noop, InputState::WaitingPaneCommand);
    }

    if input_state == InputState::WaitingPaneCommand {
        return (pane_command(key), InputState::Ready);
    }

    if input_state == InputState::WaitingGCommand {
        return match key.code {
            KeyCode::Char('g') => (EditorCommand::MoveCursor(Direction::FileStart), InputState::Ready),
            KeyCode::Char('d') => (EditorCommand::GotoDefinition, InputState::Ready),
            KeyCode::Char('t') => (EditorCommand::NextTab, InputState::Ready),
            KeyCode::Char('T') => (EditorCommand::PrevTab, InputState::Ready),
            KeyCode::Char('x') => (EditorCommand::OpenUrlUnderCursor, InputState::Ready),
            KeyCode::Char('f') => (EditorCommand::OpenFileUnderCursor, InputState::Ready),
            KeyCode::Char('v') => (EditorCommand::ReselectVisual, InputState::Ready),
            KeyCode::Char('j') => (EditorCommand::DisplayLineDown, InputState::Ready),
            KeyCode::Char('k') => (EditorCommand::DisplayLineUp, InputState::Ready),
            // gU — uppercase operator (waits for motion)
            KeyCode::Char('U') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::UpperCase)),
            // gu — lowercase operator (waits for motion)
            KeyCode::Char('u') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::LowerCase)),
            // gq — format text operator (waits for motion)
            KeyCode::Char('q') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Format)),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if input_state == InputState::WaitingZCommand {
        return match key.code {
            KeyCode::Char('a') => (EditorCommand::ToggleFold, InputState::Ready),
            KeyCode::Char('M') => (EditorCommand::FoldAll, InputState::Ready),
            KeyCode::Char('R') => (EditorCommand::UnfoldAll, InputState::Ready),
            KeyCode::Char('z') => (EditorCommand::ScrollCenter, InputState::Ready),
            KeyCode::Char('t') => (EditorCommand::ScrollTop, InputState::Ready),
            KeyCode::Char('b') => (EditorCommand::ScrollBottom, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if input_state == InputState::WaitingMarkSet {
        return match key.code {
            KeyCode::Char(c) if c.is_ascii_lowercase() => (EditorCommand::SetMark(c), InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if let InputState::WaitingMarkJump(exact) = input_state {
        return match key.code {
            // '' or `` — jump to position before last jump
            KeyCode::Char('\'') | KeyCode::Char('`') => (EditorCommand::JumpToLastPosition, InputState::Ready),
            KeyCode::Char(c) if c.is_ascii_lowercase() => (EditorCommand::JumpToMark(c, exact), InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    if input_state == InputState::WaitingRegister {
        return match key.code {
            KeyCode::Char(c) if c.is_ascii_lowercase() || c == '"' || c == '+' || c.is_ascii_digit() => {
                (EditorCommand::SelectRegister(c), InputState::Ready)
            }
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Handle WaitingFindChar state
    if let InputState::WaitingFindChar(forward, inclusive) = input_state {
        return match key.code {
            KeyCode::Char(c) => {
                if inclusive {
                    (EditorCommand::FindChar(c, forward), InputState::Ready)
                } else {
                    (EditorCommand::TillChar(c, forward), InputState::Ready)
                }
            }
            KeyCode::Esc => (EditorCommand::Noop, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Handle WaitingReplaceChar state
    if input_state == InputState::WaitingReplaceChar {
        return match key.code {
            KeyCode::Char(c) => (EditorCommand::ReplaceChar(c), InputState::Ready),
            KeyCode::Esc => (EditorCommand::Noop, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Handle WaitingIndent state (>> or <<)
    if let InputState::WaitingIndent(is_indent) = input_state {
        return match key.code {
            KeyCode::Char('>') if is_indent => (EditorCommand::Indent(1), InputState::Ready),
            KeyCode::Char('<') if !is_indent => (EditorCommand::Dedent(1), InputState::Ready),
            KeyCode::Esc => (EditorCommand::Noop, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Handle WaitingAutoIndent state (==)
    if input_state == InputState::WaitingAutoIndent {
        return match key.code {
            KeyCode::Char('=') => (EditorCommand::AutoIndent, InputState::Ready),
            _ => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Ctrl+C: cancel current operation (non-terminal), forward to PTY (terminal).
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) && !in_terminal {
        return (EditorCommand::ClearSearch, InputState::Ready);
    }

    // Ctrl+L forces full redraw (non-terminal only; terminals forward the key).
    if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) && !in_terminal {
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
        // 'i'/'a' enter text object selection
        if key.code == KeyCode::Char('i') {
            return (EditorCommand::Noop, InputState::WaitingTextObject(op, TextObjectModifier::Inner));
        }
        if key.code == KeyCode::Char('a') {
            return (EditorCommand::Noop, InputState::WaitingTextObject(op, TextObjectModifier::Around));
        }
        return (operator_motion_command(key, op), InputState::Ready);
    }

    // Handle text object kind selection (di?, ci?, da?, ca?)
    if let InputState::WaitingTextObject(op, modifier) = input_state {
        let kind = match key.code {
            KeyCode::Char('w') => Some(TextObjectKind::Word),
            KeyCode::Char('"') => Some(TextObjectKind::Quote('"')),
            KeyCode::Char('\'') => Some(TextObjectKind::Quote('\'')),
            KeyCode::Char('`') => Some(TextObjectKind::Quote('`')),
            KeyCode::Char('(') | KeyCode::Char(')') | KeyCode::Char('b') => Some(TextObjectKind::Bracket('(', ')')),
            KeyCode::Char('{') | KeyCode::Char('}') | KeyCode::Char('B') => Some(TextObjectKind::Bracket('{', '}')),
            KeyCode::Char('[') | KeyCode::Char(']') => Some(TextObjectKind::Bracket('[', ']')),
            KeyCode::Char('<') | KeyCode::Char('>') => Some(TextObjectKind::Bracket('<', '>')),
            _ => None,
        };
        return match kind {
            Some(k) => {
                let cmd = match op {
                    Operator::Delete => EditorCommand::DeleteTextObject(modifier, k),
                    Operator::Change => EditorCommand::ChangeTextObject(modifier, k),
                    Operator::Yank => EditorCommand::YankTextObject(modifier, k),
                    // gU/gu/gq + text object: not yet implemented, treat as noop
                    Operator::UpperCase | Operator::LowerCase | Operator::Format => EditorCommand::Noop,
                };
                (cmd, InputState::Ready)
            }
            None => (EditorCommand::Noop, InputState::Ready),
        };
    }

    // Handle macro register selection
    if input_state == InputState::WaitingMacroRegister {
        return match key.code {
            KeyCode::Char(':') => (EditorCommand::OpenCommandWindow, InputState::Ready),
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

    // Safety: if we reach here with any non-Ready/non-AccumulatingCount state,
    // it means an InputState variant was not handled above. Reset to Ready with
    // Noop to avoid silently misinterpreting keys in a stale waiting state.
    if input_state != InputState::Ready && input_state != InputState::AccumulatingCount {
        log::warn!(
            "key_to_command: unhandled InputState {:?} fell through to mode dispatch, resetting",
            input_state
        );
        return (EditorCommand::Noop, InputState::Ready);
    }

    match mode {
        EditorMode::Normal => {
            // When macro is recording, q/Q stops recording instead of entering WaitingMacroRegister
            if is_macro_recording && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
                return (EditorCommand::StopMacroRecord, InputState::Ready);
            }
            normal_mode_command(key)
        }
        EditorMode::Insert => (insert_mode_command(key), InputState::Ready),
        EditorMode::Replace => (replace_mode_command(key), InputState::Ready),
        EditorMode::Visual | EditorMode::VisualBlock => (visual_mode_command(key), InputState::Ready),
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
        // Pane resize / zoom
        KeyCode::Char('+') | KeyCode::Char('=') => EditorCommand::ResizePaneGrow,
        KeyCode::Char('-') => EditorCommand::ResizePaneShrink,
        KeyCode::Char('>') => EditorCommand::ResizePaneWider,
        KeyCode::Char('<') => EditorCommand::ResizePaneNarrower,
        KeyCode::Char('z') => EditorCommand::ZoomPane,
        KeyCode::Char('x') => EditorCommand::SwapPane,
        KeyCode::Char('[') => EditorCommand::EnterCopyMode,
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
        // Scroll one line: Ctrl+E down, Ctrl+Y up (viewport only, cursor stays)
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::ScrollLineDown, InputState::Ready)
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::ScrollLineUp, InputState::Ready)
        }
        // Symbol list: Ctrl+T
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::SymbolList, InputState::Ready)
        }
        // Increment / decrement number
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::IncrementNumber, InputState::Ready)
        }
        KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::DecrementNumber, InputState::Ready)
        }
        // File info: Ctrl+G
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::FileInfo, InputState::Ready)
        }
        // Full page scroll: Ctrl+B (page up), PageUp/PageDown keys
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::PageUp, InputState::Ready)
        }
        KeyCode::PageUp => (EditorCommand::PageUp, InputState::Ready),
        KeyCode::PageDown => (EditorCommand::PageDown, InputState::Ready),
        // Sentence motions
        KeyCode::Char(')') => (EditorCommand::MoveCursor(Direction::SentenceForward), InputState::Ready),
        KeyCode::Char('(') => (EditorCommand::MoveCursor(Direction::SentenceBackward), InputState::Ready),
        // Replace mode
        KeyCode::Char('R') => (EditorCommand::SwitchMode(EditorMode::Replace), InputState::Ready),
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
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::EnterVisualBlock, InputState::Ready)
        }
        KeyCode::Char('v') => (EditorCommand::EnterVisual, InputState::Ready),
        KeyCode::Char('p') => (EditorCommand::Paste, InputState::Ready),
        KeyCode::Char('P') => (EditorCommand::PasteBefore, InputState::Ready),
        KeyCode::Esc => (EditorCommand::ClearSearch, InputState::Ready),
        // Count prefix: digits 1-9 start a count, 0 is not a count starter
        KeyCode::Char(_c @ '1'..='9') => (EditorCommand::Noop, InputState::AccumulatingCount),
        // Operators: d = delete, c = change, y = yank
        KeyCode::Char('d') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Delete)),
        KeyCode::Char('c') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Change)),
        KeyCode::Char('y') => (EditorCommand::Noop, InputState::WaitingOperatorMotion(Operator::Yank)),
        // Dot repeat
        KeyCode::Char('.') => (EditorCommand::DotRepeat, InputState::Ready),
        // Register selection: "a, "+, etc.
        KeyCode::Char('"') => (EditorCommand::Noop, InputState::WaitingRegister),
        // q: stop macro recording if active, otherwise noop.
        // Use :q or Cmd+Q to quit the editor.
        // q: stop recording if active, otherwise enter macro register / command window state
        // q + letter → start recording, q + : → command window
        KeyCode::Char('q') | KeyCode::Char('Q') => (EditorCommand::Noop, InputState::WaitingMacroRegister),
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
        // Word/line/file motions
        KeyCode::Char('w') => (EditorCommand::MoveCursor(Direction::WordForward), InputState::Ready),
        KeyCode::Char('b') => (EditorCommand::MoveCursor(Direction::WordBackward), InputState::Ready),
        KeyCode::Char('e') => (EditorCommand::MoveCursor(Direction::WordEnd), InputState::Ready),
        KeyCode::Char('0') => (EditorCommand::MoveCursor(Direction::LineStart), InputState::Ready),
        KeyCode::Char('$') => (EditorCommand::MoveCursor(Direction::LineEnd), InputState::Ready),
        KeyCode::Char('G') => (EditorCommand::MoveCursor(Direction::FileEnd), InputState::Ready),
        // Screen position jumps
        KeyCode::Char('H') => (EditorCommand::ScreenTop, InputState::Ready),
        KeyCode::Char('M') => (EditorCommand::ScreenMiddle, InputState::Ready),
        KeyCode::Char('L') => (EditorCommand::ScreenBottom, InputState::Ready),
        // Y — yank to end of line (neovim behavior)
        KeyCode::Char('Y') => (EditorCommand::YankToEnd, InputState::Ready),
        // Marks
        KeyCode::Char('m') => (EditorCommand::Noop, InputState::WaitingMarkSet),
        KeyCode::Char('\'') => (EditorCommand::Noop, InputState::WaitingMarkJump(false)),
        KeyCode::Char('`') => (EditorCommand::Noop, InputState::WaitingMarkJump(true)),
        // Jump list
        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::JumpBack, InputState::Ready)
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Save, InputState::Ready)
        }
        // Character find motions
        KeyCode::Char('f') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Noop, InputState::WaitingFindChar(true, true))
        }
        KeyCode::Char('F') => (EditorCommand::Noop, InputState::WaitingFindChar(false, true)),
        KeyCode::Char('t') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Noop, InputState::WaitingFindChar(true, false))
        }
        KeyCode::Char('T') => (EditorCommand::Noop, InputState::WaitingFindChar(false, false)),
        KeyCode::Char(';') => (EditorCommand::RepeatFindChar, InputState::Ready),
        KeyCode::Char(',') => (EditorCommand::RepeatFindCharReverse, InputState::Ready),
        // Replace char
        KeyCode::Char('r') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            (EditorCommand::Noop, InputState::WaitingReplaceChar)
        }
        // Single-char operations
        KeyCode::Char('x') => (EditorCommand::DeleteCharForward, InputState::Ready),
        // Line operations
        KeyCode::Char('o') => (EditorCommand::OpenLineBelow, InputState::Ready),
        KeyCode::Char('O') => (EditorCommand::OpenLineAbove, InputState::Ready),
        KeyCode::Char('A') => (EditorCommand::AppendEndOfLine, InputState::Ready),
        KeyCode::Char('I') => (EditorCommand::InsertStartOfLine, InputState::Ready),
        KeyCode::Char('C') => (EditorCommand::ChangeToEnd, InputState::Ready),
        KeyCode::Char('D') => (EditorCommand::DeleteToEnd, InputState::Ready),
        KeyCode::Char('S') => (EditorCommand::SubstituteLine, InputState::Ready),
        KeyCode::Char('J') => (EditorCommand::JoinLines(1), InputState::Ready),
        // Case toggle
        KeyCode::Char('~') => (EditorCommand::ToggleCase, InputState::Ready),
        // Indentation
        KeyCode::Char('>') => (EditorCommand::Noop, InputState::WaitingIndent(true)),
        KeyCode::Char('<') => (EditorCommand::Noop, InputState::WaitingIndent(false)),
        // Search word under cursor
        KeyCode::Char('*') => (EditorCommand::SearchWordForward, InputState::Ready),
        KeyCode::Char('#') => (EditorCommand::SearchWordBackward, InputState::Ready),
        // Paragraph motions
        KeyCode::Char('{') => (EditorCommand::MoveCursor(Direction::ParagraphBackward), InputState::Ready),
        KeyCode::Char('}') => (EditorCommand::MoveCursor(Direction::ParagraphForward), InputState::Ready),
        // Auto-indent
        KeyCode::Char('=') => (EditorCommand::Noop, InputState::WaitingAutoIndent),
        // Match bracket
        KeyCode::Char('%') => (EditorCommand::MatchBracket, InputState::Ready),
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

fn replace_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc => EditorCommand::SwitchMode(EditorMode::Normal),
        KeyCode::Char(c) => EditorCommand::ReplaceInsertChar(c),
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
    // Helper to create the right command for the operator + direction
    let motion = |dir: Direction| -> EditorCommand {
        match op {
            Operator::Delete => EditorCommand::DeleteMotion(dir, 1),
            Operator::Change => EditorCommand::ChangeMotion(dir, 1),
            Operator::Yank => EditorCommand::YankMotion(dir, 1),
            Operator::UpperCase => EditorCommand::UpperCaseMotion(dir, 1),
            Operator::LowerCase => EditorCommand::LowerCaseMotion(dir, 1),
            Operator::Format => EditorCommand::FormatMotion(dir, 1),
        }
    };
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => motion(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => motion(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => motion(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => motion(Direction::Right),
        KeyCode::Char('w') => motion(Direction::WordForward),
        KeyCode::Char('b') => motion(Direction::WordBackward),
        KeyCode::Char('e') => motion(Direction::WordEnd),
        KeyCode::Char('0') => motion(Direction::LineStart),
        KeyCode::Char('$') => motion(Direction::LineEnd),
        // dd = delete line, cc = change line, yy = yank line
        KeyCode::Char('d') if op == Operator::Delete => EditorCommand::DeleteLines(1),
        KeyCode::Char('c') if op == Operator::Change => EditorCommand::ChangeLines(1),
        KeyCode::Char('y') if op == Operator::Yank => EditorCommand::YankLines(1),
        // gUU = uppercase line, guu = lowercase line, gqq = format line
        KeyCode::Char('U') if op == Operator::UpperCase => EditorCommand::UpperCaseLine,
        KeyCode::Char('u') if op == Operator::LowerCase => EditorCommand::LowerCaseLine,
        KeyCode::Char('q') if op == Operator::Format => EditorCommand::FormatLine,
        _ => EditorCommand::Noop,
    }
}

/// Parse a range prefix from an ex-command string.
/// Returns (optional range, remaining string).
/// Examples: "5,10s/..." → (Some((4,9)), "s/..."), "%s/..." → (None, "s/..."), "d" → (None, "d")
fn parse_range(input: &str) -> (Option<(usize, usize)>, &str) {
    // % = entire file (no specific range, use ReplaceAll)
    if let Some(rest) = input.strip_prefix('%') {
        return (None, rest);
    }
    // Try N,M prefix
    let mut chars = input.char_indices().peekable();
    let mut start_str = String::new();
    while let Some(&(_, c)) = chars.peek() {
        if c.is_ascii_digit() {
            start_str.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if !start_str.is_empty() {
        if let Some(&(_, ',')) = chars.peek() {
            chars.next(); // consume comma
            let mut end_str = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_ascii_digit() {
                    end_str.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>()) {
                let rest_idx = chars.peek().map(|&(i, _)| i).unwrap_or(input.len());
                return (Some((start.saturating_sub(1), end.saturating_sub(1))), &input[rest_idx..]);
            }
        }
    }
    (None, input)
}

fn visual_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc | KeyCode::Char('v') => EditorCommand::SwitchMode(EditorMode::Normal),
        // Movement extends selection
        KeyCode::Char('h') | KeyCode::Left => EditorCommand::MoveCursor(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => EditorCommand::MoveCursor(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => EditorCommand::MoveCursor(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => EditorCommand::MoveCursor(Direction::Right),
        // Word/line/file motions extend selection
        KeyCode::Char('w') => EditorCommand::MoveCursor(Direction::WordForward),
        KeyCode::Char('b') => EditorCommand::MoveCursor(Direction::WordBackward),
        KeyCode::Char('e') => EditorCommand::MoveCursor(Direction::WordEnd),
        KeyCode::Char('0') => EditorCommand::MoveCursor(Direction::LineStart),
        KeyCode::Char('$') => EditorCommand::MoveCursor(Direction::LineEnd),
        KeyCode::Char('G') => EditorCommand::MoveCursor(Direction::FileEnd),
        // Actions on selection
        KeyCode::Char('d') | KeyCode::Char('x') => EditorCommand::DeleteSelection,
        KeyCode::Char('y') => EditorCommand::YankSelection,
        // Indent / dedent selection
        KeyCode::Char('>') => EditorCommand::VisualIndent,
        KeyCode::Char('<') => EditorCommand::VisualDedent,
        // Case operations on selection
        KeyCode::Char('~') => EditorCommand::VisualToggleCase,
        KeyCode::Char('U') => EditorCommand::VisualUpperCase,
        KeyCode::Char('u') => EditorCommand::VisualLowerCase,
        _ => EditorCommand::Noop,
    }
}

fn command_mode_command(key: KeyEvent) -> EditorCommand {
    match key.code {
        KeyCode::Esc => EditorCommand::CommandCancel,
        KeyCode::Enter => EditorCommand::CommandExecute,
        KeyCode::Backspace => EditorCommand::CommandBackspace,
        KeyCode::Up => EditorCommand::CommandHistoryUp,
        KeyCode::Down => EditorCommand::CommandHistoryDown,
        KeyCode::Tab => EditorCommand::CommandTab,
        KeyCode::BackTab => EditorCommand::CommandTabBack,
        KeyCode::Char(c) => EditorCommand::CommandInput(c),
        _ => EditorCommand::Noop,
    }
}

/// Single source of truth for built-in ex-commands.
/// Each entry: (list of name aliases, parser function).
/// `parse_ex_command` looks up by name and calls the parser.
/// `known_commands` flattens the names for tab completion.
type CmdParser = fn(&str) -> EditorCommand;

static BUILTIN_COMMANDS: &[(&[&str], CmdParser)] = &[
    (&["w"],                    |args| {
        if let Some(cmd) = args.strip_prefix('!') {
            EditorCommand::PipeToCommand(cmd.trim().to_string())
        } else {
            EditorCommand::Save
        }
    }),
    (&["q"],                    |_| EditorCommand::Quit),
    (&["q!"],                   |_| EditorCommand::ForceQuit),
    (&["wq", "x"],              |_| EditorCommand::SaveAndQuit),
    (&["split", "sp"],          |_| EditorCommand::SplitPane(SplitDirection::Horizontal)),
    (&["vsplit", "vs"],         |_| EditorCommand::SplitPane(SplitDirection::Vertical)),
    (&["terminal", "term"],     |_| EditorCommand::OpenTerminal),
    (&["e", "edit"],            |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::EditFile(args.to_string()) }
    }),
    (&["mksession"],            |args| {
        let name = if args.is_empty() { "default" } else { args };
        EditorCommand::SaveSession(name.to_string())
    }),
    (&["close"],                |_| EditorCommand::ClosePane),
    (&["bnext", "bn"],          |_| EditorCommand::BufferNext),
    (&["bprev", "bp"],          |_| EditorCommand::BufferPrev),
    (&["ls", "buffers"],        |_| EditorCommand::BufferList),
    (&["find", "Files"],        |args| {
        if args.is_empty() { EditorCommand::OpenFileFinder }
        else { EditorCommand::OpenFinderAt(args.to_string()) }
    }),
    (&["tabnew", "tabe"],       |args| {
        let dir = if args.is_empty() { "." } else { args };
        EditorCommand::OpenTab(dir.to_string())
    }),
    (&["tabn", "tabnext"],      |_| EditorCommand::NextTab),
    (&["tabp", "tabprev"],      |_| EditorCommand::PrevTab),
    (&["tabclose", "tabc"],     |_| EditorCommand::CloseTab),
    (&["tabs", "workspaces"],   |_| EditorCommand::ListWorkspaces),
    (&["tabrename"],            |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::RenameTab(args.to_string()) }
    }),
    (&["cd", "lcd", "chdir"],   |args| {
        let dir = if args.is_empty() {
            std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
        } else if let Some(rest) = args.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{}/{}", home, rest)
        } else {
            args.to_string()
        };
        EditorCommand::ChangeDirectory(dir)
    }),
    (&["read", "r"],            |args| {
        if let Some(cmd) = args.strip_prefix('!') {
            EditorCommand::ReadCommand(cmd.trim().to_string())
        } else if !args.is_empty() {
            EditorCommand::ReadFile(args.to_string())
        } else {
            EditorCommand::Noop
        }
    }),
    (&["sort"],                 |_| EditorCommand::SortLines),
    (&["marks"],                |_| EditorCommand::ListMarks),
    (&["registers", "reg"],     |_| EditorCommand::ListRegisters),
    (&["noh", "nohlsearch"],    |_| EditorCommand::ClearSearch),
    (&["definition", "def"],    |_| EditorCommand::GotoDefinition),
    (&["explore", "Explore", "Ex"], |args| {
        if args.is_empty() { EditorCommand::ToggleExplorer }
        else { EditorCommand::OpenExplorer(args.to_string()) }
    }),
    (&["undo"],                 |_| EditorCommand::Undo),
    (&["redo"],                 |_| EditorCommand::Redo),
    (&["help", "h"],            |_| EditorCommand::ToggleHelp),
    (&["echo"],                 |args| EditorCommand::Echo(args.to_string())),
    (&["PluginList", "pluginlist", "plugins"], |_| EditorCommand::PluginList),
    (&["source", "so"],         |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::SourceFile(args.to_string()) }
    }),
    (&["set"],                  |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::SetOption(args.to_string()) }
    }),
    (&["copen", "cope"],        |_| EditorCommand::QuickfixOpen),
    (&["cnext", "cn"],          |_| EditorCommand::QuickfixNext),
    (&["cprev", "cp", "cN"],    |_| EditorCommand::QuickfixPrev),
    (&["cclose", "ccl"],        |_| EditorCommand::QuickfixClose),
    (&["symbols", "Symbols"],   |_| EditorCommand::SymbolList),
    (&["blame", "Gblame"],      |_| EditorCommand::ToggleBlame),
    (&["minimap"],              |_| EditorCommand::ToggleMinimap),
    (&["outline", "Outline"],   |_| EditorCommand::ToggleOutline),
    (&["diff", "Gdiff"],        |_| EditorCommand::GitDiff),
    (&["PlugInstall"],          |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::PluginInstall(args.to_string()) }
    }),
    (&["PlugUpdate"],           |args| EditorCommand::PluginUpdate(args.to_string())),
    (&["PlugRemove"],           |args| {
        if args.is_empty() { EditorCommand::Noop }
        else { EditorCommand::PluginRemove(args.to_string()) }
    }),
    (&["make"],                 |args| {
        let cmd = if args.is_empty() { "make" } else { args };
        EditorCommand::Make(cmd.to_string())
    }),
];

/// All built-in ex-command names (for tab completion).
/// Derived from the single-source-of-truth `BUILTIN_COMMANDS` table.
pub fn known_commands() -> Vec<&'static str> {
    BUILTIN_COMMANDS.iter()
        .flat_map(|(names, _)| names.iter().copied())
        .collect()
}

/// Parse and convert an ex-command string into an EditorCommand.
/// Looks up the command name in `BUILTIN_COMMANDS`; falls through to
/// range/substitution parsing and plugin commands if not found.
pub fn parse_ex_command(input: &str) -> EditorCommand {
    let input = input.trim();

    // :! shell command
    if let Some(cmd) = input.strip_prefix('!') {
        return EditorCommand::ShellCommand(cmd.trim().to_string());
    }

    let (cmd, args) = match input.split_once(' ') {
        Some((c, a)) => (c, a.trim()),
        None => (input, ""),
    };

    // Look up in the data-driven command table
    for (names, parser) in BUILTIN_COMMANDS {
        if names.contains(&cmd) {
            return parser(args);
        }
    }

    // Fallthrough: range commands, substitution, plugin commands
    let (range, rest) = parse_range(input);

    // Handle s/pattern/replacement/[flags]
    if let Some(sub_rest) = rest.strip_prefix("s/") {
        let parts: Vec<&str> = sub_rest.splitn(3, '/').collect();
        if parts.len() >= 2 {
            let pattern = parts[0].to_string();
            let replacement = parts.get(1).unwrap_or(&"").to_string();
            let flags = parts.get(2).unwrap_or(&"");
            let case_insensitive = flags.contains('i');
            let confirm = flags.contains('c');
            if confirm {
                return EditorCommand::ReplaceConfirm(pattern, replacement, case_insensitive);
            }
            return EditorCommand::ReplaceAll(pattern, replacement, case_insensitive);
        }
    }

    // Handle range + d (e.g., 5,10d)
    if rest == "d" {
        if let Some((start, end)) = range {
            return EditorCommand::DeleteLines(end.saturating_sub(start) + 1);
        }
    }

    // Fall through to plugin registry for unknown commands
    EditorCommand::PluginCommand(cmd.to_string(), args.to_string())
}
