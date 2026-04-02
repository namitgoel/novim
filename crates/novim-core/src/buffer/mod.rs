//! Text buffer management
//!
//! Uses Ropey for efficient rope-based text storage and manipulation.

use novim_types::{Direction, Position, Selection};
use ropey::Rope;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::NovimError;
use crate::fold::FoldState;
use crate::highlight::{HighlightSpan, SyntaxHighlighter};

// --- Focused sub-traits for BufferLike ---

/// Core display and navigation shared by all pane content.
pub trait PaneDisplay {
    fn cursor(&self) -> Position;
    fn move_cursor(&mut self, direction: Direction);
    fn set_cursor_pos(&mut self, _pos: Position) {}
    fn get_line(&self, line: usize) -> Option<String>;
    fn len_lines(&self) -> usize;
    fn display_name(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn selection(&self) -> Option<Selection> { None }
    fn set_selection(&mut self, _sel: Option<Selection>) {}
    fn selected_text(&self) -> Option<String> { None }
    fn get_highlights(&self, _line: usize) -> Option<&[HighlightSpan]> { None }
    fn reparse_highlights(&mut self) {}
    fn secondary_cursors(&self) -> &[Position] { &[] }
    fn add_cursor_above(&mut self) {}
    fn add_cursor_below(&mut self) {}
    fn clear_secondary_cursors(&mut self) {}
    fn fold_state(&self) -> Option<&FoldState> { None }
    fn toggle_fold(&mut self, _line: usize) -> bool { false }
    fn fold_all(&mut self) {}
    fn unfold_all(&mut self) {}
    fn recompute_folds(&mut self, _tab_width: usize) {}
}

/// Text editing operations (insert, delete, undo/redo, save).
pub trait TextEditing {
    fn insert_char(&mut self, _c: char) {}
    fn insert_tab(&mut self, _tab_width: usize, _expand_tab: bool) {}
    fn insert_newline_with_indent(&mut self, _auto_indent: bool) {
        self.insert_char('\n');
    }
    fn delete_char_before_cursor(&mut self) {}
    fn delete_lines(&mut self, _n: usize) -> Option<String> { None }
    fn delete_motion(&mut self, _dir: Direction, _n: usize) {}
    fn delete_selection(&mut self) -> Option<String> { None }
    fn undo(&mut self) -> Option<String> { None }
    fn redo(&mut self) -> Option<String> { None }
    fn break_undo_group(&mut self) {}
    fn save(&mut self) -> std::result::Result<String, NovimError> {
        Err(NovimError::Buffer("Not supported".to_string()))
    }
}

/// Search and replace operations.
pub trait Searchable {
    fn search_forward(&self, _pattern: &str, _from: Position) -> Option<Position> { None }
    fn search_backward(&self, _pattern: &str, _from: Position) -> Option<Position> { None }
    fn replace_all(&mut self, _pattern: &str, _replacement: &str) -> usize { 0 }
}

/// Terminal-specific operations.
pub trait TerminalLike {
    fn send_key(&mut self, _key: crossterm::event::KeyEvent) {}
    fn is_terminal(&self) -> bool { false }
    fn poll_pty(&mut self) -> bool { false }
    fn get_styled_cells(&self, _row: usize) -> Option<&[crate::emulator::grid::Cell]> { None }
    fn shell_cwd(&self) -> Option<std::path::PathBuf> { None }
}

/// Unified trait combining all sub-traits. All pane content implements this.
/// Blanket-implemented for any type that implements all four sub-traits.
pub trait BufferLike: PaneDisplay + TextEditing + Searchable + TerminalLike {}
impl<T: PaneDisplay + TextEditing + Searchable + TerminalLike> BufferLike for T {}

/// A single edit operation (for undo/redo).
#[derive(Debug, Clone)]
enum EditOp {
    /// Characters were inserted at this char index
    Insert { char_idx: usize, content: String },
    /// Characters were deleted from this char index
    Delete { char_idx: usize, content: String },
}

/// A group of edits that undo/redo together (e.g., typing a word).
#[derive(Debug, Clone)]
struct UndoGroup {
    ops: Vec<EditOp>,
    /// Cursor position before this group was applied
    cursor_before: Position,
}

/// A text buffer backed by a Rope
pub struct Buffer {
    rope: Rope,
    cursor: Position,
    secondary_cursors: Vec<Position>,
    dirty: bool,
    file_path: Option<PathBuf>,
    undo_stack: Vec<UndoGroup>,
    redo_stack: Vec<UndoGroup>,
    current_group: Option<UndoGroup>,
    selection: Option<Selection>,
    highlighter: Option<SyntaxHighlighter>,
    cached_highlights: Vec<Vec<HighlightSpan>>,
    highlights_dirty: bool,
    /// Cached string representation of the rope, invalidated on edits.
    cached_text: Option<String>,
    /// Code folding state
    folds: FoldState,
    /// Document version counter for LSP (incremented on every edit)
    version: i32,
}

impl Buffer {
    /// Create a new empty buffer
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            cursor: Position::zero(),
            secondary_cursors: Vec::new(),
            dirty: false,
            file_path: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: None,
            selection: None,
            highlighter: None,
            cached_highlights: Vec::new(),
            highlights_dirty: true,
            folds: FoldState::new(),
            cached_text: None,
            version: 0,
        }
    }

    /// Load a buffer from a file, or create empty buffer if file doesn't exist
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let highlighter = path_buf
            .extension()
            .and_then(|e| e.to_str())
            .and_then(SyntaxHighlighter::from_extension);

        let rope = match fs::read_to_string(&path) {
            Ok(content) => Rope::from_str(&content),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Rope::new(),
            Err(e) => return Err(e),
        };

        Ok(Self {
            rope,
            cursor: Position::zero(),
            secondary_cursors: Vec::new(),
            dirty: false,
            file_path: Some(path_buf),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: None,
            selection: None,
            highlighter,
            cached_highlights: Vec::new(),
            highlights_dirty: true,
            folds: FoldState::new(),
            cached_text: None,
            version: 0,
        })
    }

    pub fn file_path_str(&self) -> Option<&str> {
        self.file_path.as_deref().and_then(|p| p.to_str())
    }

    /// Get the full document text (for LSP didOpen/didChange).
    pub fn full_text(&self) -> String {
        self.rope.to_string()
    }

    /// Get cached text representation, materializing from rope only if dirty.
    fn text_cache(&mut self) -> &str {
        if self.cached_text.is_none() {
            self.cached_text = Some(self.rope.to_string());
        }
        self.cached_text.as_ref().unwrap()
    }

    fn invalidate_text_cache(&mut self) {
        self.cached_text = None;
    }

    /// Get text as a String, using the cache if available or materializing from rope.
    fn get_text(&self) -> std::borrow::Cow<'_, str> {
        match &self.cached_text {
            Some(s) => std::borrow::Cow::Borrowed(s.as_str()),
            None => std::borrow::Cow::Owned(self.rope.to_string()),
        }
    }

    /// Get the document version (incremented on every edit).
    pub fn version(&self) -> i32 {
        self.version
    }

    /// Get the file as a URI (file:///path) for LSP.
    pub fn file_uri(&self) -> Option<String> {
        self.file_path.as_ref().and_then(|p| {
            p.canonicalize().ok().or_else(|| Some(p.clone()))
        }).map(|p| format!("file://{}", p.display()))
    }

    pub fn set_cursor_position(&mut self, line: usize, col: usize) {
        let max_line = self.rope.len_lines().saturating_sub(1);
        self.cursor.line = line.min(max_line);
        self.clamp_cursor_column();
        self.cursor.column = col;
        self.clamp_cursor_column();
    }

    fn cursor_to_char_idx(&self) -> usize {
        self.rope.line_to_char(self.cursor.line) + self.cursor.column
    }

    fn line_len(&self, line: usize) -> usize {
        if line < self.rope.len_lines() {
            let line_content = self.rope.line(line);
            let len = line_content.len_chars();
            if len > 0 && line_content.char(len - 1) == '\n' {
                len - 1
            } else {
                len
            }
        } else {
            0
        }
    }

    fn clamp_cursor_column(&mut self) {
        let max_col = self.line_len(self.cursor.line);
        if self.cursor.column > max_col {
            self.cursor.column = max_col;
        }
    }

    /// Ensure a current undo group exists, creating one if needed.
    fn ensure_undo_group(&mut self) {
        if self.current_group.is_none() {
            self.current_group = Some(UndoGroup {
                ops: Vec::new(),
                cursor_before: self.cursor,
            });
        }
    }

    /// Flush the current undo group to the undo stack.
    fn flush_undo_group(&mut self) {
        if let Some(group) = self.current_group.take() {
            if !group.ops.is_empty() {
                self.undo_stack.push(group);
            }
        }
    }

    /// Compute cursor position from a char index.
    fn char_idx_to_position(&self, char_idx: usize) -> Position {
        let line = self.rope.char_to_line(char_idx);
        let line_start = self.rope.line_to_char(line);
        Position::new(line, char_idx - line_start)
    }

    /// Classify a character: 0 = whitespace, 1 = word (alnum/_), 2 = punctuation
    fn char_class(c: char) -> u8 {
        if c.is_whitespace() { 0 }
        else if c.is_alphanumeric() || c == '_' { 1 }
        else { 2 }
    }

    /// Find position after moving forward one word (vim `w`).
    fn find_word_forward_pos(&self) -> Position {
        let total = self.rope.len_chars();
        let mut idx = self.cursor_to_char_idx();
        if idx >= total { return self.cursor; }

        // Skip current word class
        let start_class = Self::char_class(self.rope.char(idx));
        while idx < total && Self::char_class(self.rope.char(idx)) == start_class {
            idx += 1;
        }
        // Skip whitespace
        while idx < total && self.rope.char(idx).is_whitespace() && self.rope.char(idx) != '\n' {
            idx += 1;
        }
        // If we hit a newline, move past it to the next line
        if idx < total && self.rope.char(idx) == '\n' {
            idx += 1;
            // Skip blank lines
            while idx < total && self.rope.char(idx) == '\n' {
                idx += 1;
            }
        }
        if idx >= total { idx = total.saturating_sub(1); }
        self.char_idx_to_position(idx)
    }

    /// Find position after moving backward one word (vim `b`).
    fn find_word_backward_pos(&self) -> Position {
        let mut idx = self.cursor_to_char_idx();
        if idx == 0 { return self.cursor; }
        idx -= 1;

        // Skip whitespace/newlines backward
        while idx > 0 && (self.rope.char(idx).is_whitespace()) {
            idx -= 1;
        }
        // Skip current word class backward
        let target_class = Self::char_class(self.rope.char(idx));
        while idx > 0 && Self::char_class(self.rope.char(idx - 1)) == target_class {
            idx -= 1;
        }
        self.char_idx_to_position(idx)
    }

    /// Find position at end of current/next word (vim `e`).
    fn find_word_end_pos(&self) -> Position {
        let total = self.rope.len_chars();
        let mut idx = self.cursor_to_char_idx();
        if idx >= total.saturating_sub(1) { return self.cursor; }
        idx += 1;

        // Skip whitespace
        while idx < total && self.rope.char(idx).is_whitespace() {
            idx += 1;
        }
        if idx >= total { return self.char_idx_to_position(total.saturating_sub(1)); }
        // Advance through current word class
        let target_class = Self::char_class(self.rope.char(idx));
        while idx + 1 < total && Self::char_class(self.rope.char(idx + 1)) == target_class {
            idx += 1;
        }
        self.char_idx_to_position(idx)
    }
}

impl PaneDisplay for Buffer {
    fn cursor(&self) -> Position {
        self.cursor
    }

    fn move_cursor(&mut self, direction: Direction) {
        match direction {
            Direction::Left => {
                if self.cursor.column > 0 {
                    self.cursor.column -= 1;
                }
            }
            Direction::Right => {
                let line_len = self.line_len(self.cursor.line);
                if self.cursor.column < line_len {
                    self.cursor.column += 1;
                }
            }
            Direction::Up => {
                if self.cursor.line > 0 {
                    self.cursor.line = self.folds.prev_visible_line(self.cursor.line);
                    self.clamp_cursor_column();
                }
            }
            Direction::Down => {
                let total = self.rope.len_lines();
                let next = self.folds.next_visible_line(self.cursor.line, total);
                if next < total {
                    self.cursor.line = next;
                    self.clamp_cursor_column();
                }
            }
            Direction::WordForward => {
                self.cursor = self.find_word_forward_pos();
            }
            Direction::WordBackward => {
                self.cursor = self.find_word_backward_pos();
            }
            Direction::WordEnd => {
                self.cursor = self.find_word_end_pos();
            }
            Direction::LineStart => {
                self.cursor.column = 0;
            }
            Direction::LineEnd => {
                self.cursor.column = self.line_len(self.cursor.line);
            }
            Direction::FileStart => {
                self.cursor = Position::zero();
            }
            Direction::FileEnd => {
                self.cursor.line = self.rope.len_lines().saturating_sub(1);
                self.clamp_cursor_column();
            }
        }
    }

    fn get_line(&self, line: usize) -> Option<String> {
        if line < self.rope.len_lines() {
            Some(
                self.rope
                    .line(line)
                    .to_string()
                    .trim_end_matches('\n')
                    .to_string(),
            )
        } else {
            None
        }
    }

    fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("[No Name]")
            .to_string()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn selection(&self) -> Option<Selection> {
        self.selection
    }

    fn set_selection(&mut self, sel: Option<Selection>) {
        self.selection = sel;
    }

    fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (start, end) = sel.ordered();
        let start_idx = self.rope.line_to_char(start.line) + start.column;
        let end_idx = self.rope.line_to_char(end.line) + end.column + 1;
        let end_idx = end_idx.min(self.rope.len_chars());
        Some(self.rope.slice(start_idx..end_idx).to_string())
    }

    fn set_cursor_pos(&mut self, pos: Position) {
        self.set_cursor_position(pos.line, pos.column);
    }

    fn get_highlights(&self, line: usize) -> Option<&[HighlightSpan]> {
        self.cached_highlights.get(line).map(|v| v.as_slice())
    }

    fn reparse_highlights(&mut self) {
        if !self.highlights_dirty {
            return;
        }
        let _ = self.text_cache();
        if let Some(hl) = &self.highlighter {
            let source = self.cached_text.as_deref().unwrap();
            self.cached_highlights = hl.highlight(source);
        }
        self.highlights_dirty = false;
    }

    fn secondary_cursors(&self) -> &[Position] {
        &self.secondary_cursors
    }

    fn add_cursor_above(&mut self) {
        if self.cursor.line > 0 {
            let new = Position::new(self.cursor.line - 1, self.cursor.column);
            if !self.secondary_cursors.contains(&new) && new != self.cursor {
                self.secondary_cursors.push(new);
                self.secondary_cursors.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
            }
        }
    }

    fn add_cursor_below(&mut self) {
        if self.cursor.line + 1 < self.rope.len_lines() {
            let new = Position::new(self.cursor.line + 1, self.cursor.column);
            if !self.secondary_cursors.contains(&new) && new != self.cursor {
                self.secondary_cursors.push(new);
                self.secondary_cursors.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
            }
        }
    }

    fn clear_secondary_cursors(&mut self) {
        self.secondary_cursors.clear();
    }

    fn fold_state(&self) -> Option<&FoldState> {
        if self.folds.has_folds() { Some(&self.folds) } else { None }
    }

    fn toggle_fold(&mut self, line: usize) -> bool {
        self.folds.toggle_fold(line)
    }

    fn fold_all(&mut self) {
        self.folds.fold_all();
    }

    fn unfold_all(&mut self) {
        self.folds.unfold_all();
    }

    fn recompute_folds(&mut self, tab_width: usize) {
        let lines: Vec<String> = (0..self.rope.len_lines())
            .map(|i| self.get_line(i).unwrap_or_default())
            .collect();
        self.folds = FoldState::detect_indent_folds(&lines, tab_width);
    }
}

impl TextEditing for Buffer {
    fn insert_char(&mut self, c: char) {
        // Collect all cursor positions (primary + secondary), sorted in reverse document order
        // so that insertions at later positions don't shift earlier ones.
        let mut all_cursors: Vec<(usize, Position)> = Vec::new();
        all_cursors.push((usize::MAX, self.cursor)); // primary marked with MAX
        for (i, &pos) in self.secondary_cursors.iter().enumerate() {
            all_cursors.push((i, pos));
        }
        all_cursors.sort_by(|a, b| b.1.line.cmp(&a.1.line).then(b.1.column.cmp(&a.1.column)));

        for &(idx, pos) in &all_cursors {
            let char_idx = self.rope.line_to_char(pos.line) + pos.column;
            self.rope.insert_char(char_idx, c);

            self.ensure_undo_group();
            if let Some(group) = &mut self.current_group {
                group.ops.push(EditOp::Insert { char_idx, content: c.to_string() });
            }

            let new_pos = if c == '\n' {
                Position::new(pos.line + 1, 0)
            } else {
                Position::new(pos.line, pos.column + 1)
            };

            if idx == usize::MAX {
                self.cursor = new_pos;
            } else {
                self.secondary_cursors[idx] = new_pos;
            }
        }

        self.dirty = true;
        self.highlights_dirty = true;
        self.version += 1;
        self.invalidate_text_cache();
        self.redo_stack.clear();
    }

    fn insert_tab(&mut self, tab_width: usize, expand_tab: bool) {
        if expand_tab {
            let col = self.cursor.column;
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces {
                self.insert_char(' ');
            }
        } else {
            self.insert_char('\t');
        }
    }

    fn insert_newline_with_indent(&mut self, auto_indent: bool) {
        let indent = if auto_indent {
            self.get_line(self.cursor.line)
                .map(|line| {
                    let ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    ws
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.insert_char('\n');
        for c in indent.chars() {
            self.insert_char(c);
        }
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
            let char_idx = self.cursor_to_char_idx();
            let deleted = self.rope.char(char_idx).to_string();
            self.rope.remove(char_idx..char_idx + 1);
            self.dirty = true;
            self.highlights_dirty = true;
            self.version += 1;
            self.invalidate_text_cache();

            self.ensure_undo_group();
            if let Some(group) = &mut self.current_group {
                group.ops.push(EditOp::Delete {
                    char_idx,
                    content: deleted,
                });
            }
            self.redo_stack.clear();
        } else if self.cursor.line > 0 {
            let prev_line_len = self.line_len(self.cursor.line - 1);
            self.cursor.line -= 1;
            self.cursor.column = prev_line_len;
            let char_idx = self.cursor_to_char_idx();
            let deleted = self.rope.char(char_idx).to_string();
            self.rope.remove(char_idx..char_idx + 1);
            self.dirty = true;
            self.highlights_dirty = true;
            self.version += 1;
            self.invalidate_text_cache();

            self.ensure_undo_group();
            if let Some(group) = &mut self.current_group {
                group.ops.push(EditOp::Delete {
                    char_idx,
                    content: deleted,
                });
            }
            self.redo_stack.clear();
        }
    }

    fn save(&mut self) -> std::result::Result<String, NovimError> {
        if let Some(path) = &self.file_path {
            fs::write(path, self.rope.to_string())?;
            self.dirty = false;
            Ok(format!("\"{}\" written", path.display()))
        } else {
            Err(NovimError::Buffer("No file path set".to_string()))
        }
    }

    fn undo(&mut self) -> Option<String> {
        // Flush any pending edits first
        self.flush_undo_group();

        let group = self.undo_stack.pop()?;
        let cursor_after = self.cursor;

        // Apply ops in reverse order
        for op in group.ops.iter().rev() {
            match op {
                EditOp::Insert { char_idx, content } => {
                    // Undo an insert = delete
                    self.rope.remove(*char_idx..*char_idx + content.len());
                }
                EditOp::Delete { char_idx, content } => {
                    // Undo a delete = insert
                    self.rope.insert(*char_idx, content);
                }
            }
        }

        // Restore cursor to before the group
        self.cursor = group.cursor_before;
        self.dirty = !self.undo_stack.is_empty();
        self.invalidate_text_cache();

        // Push to redo (with cursor_before = current position for redo to restore)
        self.redo_stack.push(UndoGroup {
            ops: group.ops,
            cursor_before: cursor_after,
        });

        Some(format!(
            "Undo ({} remaining)",
            self.undo_stack.len()
        ))
    }

    fn redo(&mut self) -> Option<String> {
        let group = self.redo_stack.pop()?;
        let cursor_before = self.cursor;

        // Apply ops in forward order
        for op in &group.ops {
            match op {
                EditOp::Insert { char_idx, content } => {
                    self.rope.insert(*char_idx, content);
                }
                EditOp::Delete { char_idx, content } => {
                    self.rope.remove(*char_idx..*char_idx + content.len());
                }
            }
        }

        // Restore cursor to after the group
        self.cursor = group.cursor_before;
        self.dirty = true;
        self.invalidate_text_cache();

        // Push back to undo
        self.undo_stack.push(UndoGroup {
            ops: group.ops,
            cursor_before,
        });

        Some(format!(
            "Redo ({} remaining)",
            self.redo_stack.len()
        ))
    }

    fn break_undo_group(&mut self) {
        self.flush_undo_group();
    }

    fn delete_lines(&mut self, n: usize) -> Option<String> {
        if self.rope.len_chars() == 0 {
            return None;
        }

        let start_line = self.cursor.line;
        let end_line = (start_line + n).min(self.rope.len_lines());
        let start_idx = self.rope.line_to_char(start_line);
        let end_idx = if end_line >= self.rope.len_lines() {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(end_line)
        };

        if start_idx >= end_idx {
            return None;
        }

        let deleted = self.rope.slice(start_idx..end_idx).to_string();

        self.ensure_undo_group();
        if let Some(group) = &mut self.current_group {
            group.ops.push(EditOp::Delete {
                char_idx: start_idx,
                content: deleted.clone(),
            });
        }
        self.redo_stack.clear();

        self.rope.remove(start_idx..end_idx);
        self.dirty = true;
        self.highlights_dirty = true;
        self.version += 1;
        self.invalidate_text_cache();

        let max_line = self.rope.len_lines().saturating_sub(1);
        if self.cursor.line > max_line {
            self.cursor.line = max_line;
        }
        self.clamp_cursor_column();

        Some(deleted)
    }

    fn delete_motion(&mut self, dir: Direction, n: usize) {
        for _ in 0..n {
            match dir {
                Direction::Left => {
                    self.delete_char_before_cursor();
                }
                Direction::Right => {
                    let char_idx = self.cursor_to_char_idx();
                    if char_idx < self.rope.len_chars() {
                        let deleted = self.rope.char(char_idx).to_string();
                        self.ensure_undo_group();
                        if let Some(group) = &mut self.current_group {
                            group.ops.push(EditOp::Delete {
                                char_idx,
                                content: deleted,
                            });
                        }
                        self.redo_stack.clear();
                        self.rope.remove(char_idx..char_idx + 1);
                        self.dirty = true;
                        self.highlights_dirty = true;
                        self.version += 1;
                        self.invalidate_text_cache();
                    }
                }
                Direction::Down => {
                    self.delete_lines(1);
                }
                Direction::Up => {
                    if self.cursor.line > 0 {
                        self.cursor.line -= 1;
                        self.delete_lines(1);
                    }
                }
                // Word/line/file motions: delete from cursor to target position
                Direction::WordForward | Direction::WordBackward | Direction::WordEnd
                | Direction::LineStart | Direction::LineEnd
                | Direction::FileStart | Direction::FileEnd => {
                    let start_idx = self.cursor_to_char_idx();
                    self.move_cursor(dir);
                    let end_idx = self.cursor_to_char_idx();
                    let (from, to) = if start_idx <= end_idx {
                        (start_idx, end_idx)
                    } else {
                        (end_idx, start_idx)
                    };
                    if from < to && to <= self.rope.len_chars() {
                        let deleted = self.rope.slice(from..to).to_string();
                        self.ensure_undo_group();
                        if let Some(group) = &mut self.current_group {
                            group.ops.push(EditOp::Delete {
                                char_idx: from,
                                content: deleted,
                            });
                        }
                        self.redo_stack.clear();
                        self.rope.remove(from..to);
                        self.cursor = self.char_idx_to_position(from);
                        self.dirty = true;
                        self.highlights_dirty = true;
                        self.version += 1;
                        self.invalidate_text_cache();
                    }
                }
            }
        }
    }

    fn delete_selection(&mut self) -> Option<String> {
        let sel = self.selection.take()?;
        let (start, end) = sel.ordered();
        let start_idx = self.rope.line_to_char(start.line) + start.column;
        let end_idx = self.rope.line_to_char(end.line) + end.column + 1;
        let end_idx = end_idx.min(self.rope.len_chars());

        let deleted = self.rope.slice(start_idx..end_idx).to_string();

        self.ensure_undo_group();
        if let Some(group) = &mut self.current_group {
            group.ops.push(EditOp::Delete {
                char_idx: start_idx,
                content: deleted.clone(),
            });
        }
        self.redo_stack.clear();

        self.rope.remove(start_idx..end_idx);
        self.cursor = start;
        self.dirty = true;
        self.invalidate_text_cache();

        Some(deleted)
    }
}

impl Searchable for Buffer {
    fn search_forward(&self, pattern: &str, from: Position) -> Option<Position> {
        if pattern.is_empty() {
            return None;
        }
        let start_idx = self.rope.line_to_char(from.line) + from.column;
        let text = self.get_text();

        // Try regex first, fall back to literal
        if let Ok(re) = regex::Regex::new(pattern) {
            // Search forward from cursor
            if let Some(m) = re.find(&text[start_idx + 1..]) {
                return Some(self.char_idx_to_position(start_idx + 1 + m.start()));
            }
            // Wrap around
            if let Some(m) = re.find(&text[..start_idx]) {
                return Some(self.char_idx_to_position(m.start()));
            }
        } else {
            // Invalid regex — use literal search
            if let Some(pos) = text[start_idx + 1..].find(pattern) {
                return Some(self.char_idx_to_position(start_idx + 1 + pos));
            }
            if let Some(pos) = text[..start_idx].find(pattern) {
                return Some(self.char_idx_to_position(pos));
            }
        }
        None
    }

    fn search_backward(&self, pattern: &str, from: Position) -> Option<Position> {
        if pattern.is_empty() {
            return None;
        }
        let start_idx = self.rope.line_to_char(from.line) + from.column;
        let text = self.get_text();

        if let Ok(re) = regex::Regex::new(pattern) {
            // Find last match before cursor
            let mut last_match = None;
            for m in re.find_iter(&text[..start_idx]) {
                last_match = Some(m.start());
            }
            if let Some(pos) = last_match {
                return Some(self.char_idx_to_position(pos));
            }
            // Wrap around — find last match after cursor
            let mut last_match = None;
            for m in re.find_iter(&text[start_idx..]) {
                last_match = Some(start_idx + m.start());
            }
            if let Some(pos) = last_match {
                return Some(self.char_idx_to_position(pos));
            }
        } else {
            if let Some(pos) = text[..start_idx].rfind(pattern) {
                return Some(self.char_idx_to_position(pos));
            }
            if let Some(pos) = text[start_idx..].rfind(pattern) {
                return Some(self.char_idx_to_position(start_idx + pos));
            }
        }
        None
    }

    fn replace_all(&mut self, pattern: &str, replacement: &str) -> usize {
        if pattern.is_empty() {
            return 0;
        }
        let text = self.text_cache().to_string();

        // Try regex replace, fall back to literal
        let (new_text, count) = if let Ok(re) = regex::Regex::new(pattern) {
            let count = re.find_iter(&text).count();
            if count == 0 {
                return 0;
            }
            (re.replace_all(&text, replacement).to_string(), count)
        } else {
            let count = text.matches(pattern).count();
            if count == 0 {
                return 0;
            }
            (text.replace(pattern, replacement), count)
        };

        // Record the entire replacement as one undo group
        self.flush_undo_group();
        let old_text = text;

        self.rope = Rope::from_str(&new_text);
        self.dirty = true;
        self.invalidate_text_cache();

        // Record undo as delete old + insert new
        self.undo_stack.push(UndoGroup {
            cursor_before: self.cursor,
            ops: vec![
                EditOp::Delete {
                    char_idx: 0,
                    content: old_text,
                },
                EditOp::Insert {
                    char_idx: 0,
                    content: new_text,
                },
            ],
        });
        self.redo_stack.clear();

        // Clamp cursor
        let max_line = self.rope.len_lines().saturating_sub(1);
        if self.cursor.line > max_line {
            self.cursor.line = max_line;
        }
        self.clamp_cursor_column();

        count
    }
}

/// Buffer uses default (no-op) terminal operations.
impl TerminalLike for Buffer {}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buffer = Buffer::new();
        assert_eq!(buffer.cursor(), Position::zero());
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn test_insert_char() {
        let mut buffer = Buffer::new();
        buffer.insert_char('h');
        buffer.insert_char('i');
        assert_eq!(buffer.get_line(0), Some("hi".to_string()));
        assert!(buffer.is_dirty());
        assert_eq!(buffer.cursor(), Position::new(0, 2));
    }

    #[test]
    fn test_insert_newline() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.insert_char('\n');
        buffer.insert_char('b');
        assert_eq!(buffer.get_line(0), Some("a".to_string()));
        assert_eq!(buffer.get_line(1), Some("b".to_string()));
        assert_eq!(buffer.cursor(), Position::new(1, 1));
    }

    #[test]
    fn test_cursor_movement() {
        let mut buffer = Buffer::new();
        buffer.insert_char('h');
        buffer.insert_char('e');
        buffer.insert_char('l');
        buffer.insert_char('l');
        buffer.insert_char('o');

        assert_eq!(buffer.cursor(), Position::new(0, 5));
        buffer.move_cursor(Direction::Left);
        assert_eq!(buffer.cursor(), Position::new(0, 4));
        buffer.move_cursor(Direction::Right);
        assert_eq!(buffer.cursor(), Position::new(0, 5));
    }

    #[test]
    fn test_delete_char() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.insert_char('b');
        buffer.insert_char('c');

        buffer.delete_char_before_cursor();
        assert_eq!(buffer.get_line(0), Some("ab".to_string()));
        assert_eq!(buffer.cursor(), Position::new(0, 2));
    }

    #[test]
    fn test_line_to_char_uses_ropey_api() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.insert_char('b');
        buffer.insert_char('\n');
        buffer.insert_char('c');
        buffer.insert_char('d');

        assert_eq!(buffer.cursor(), Position::new(1, 2));
        assert_eq!(buffer.cursor_to_char_idx(), 5);
    }

    #[test]
    fn test_undo_insert() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.insert_char('b');
        buffer.insert_char('c');
        assert_eq!(buffer.get_line(0), Some("abc".to_string()));

        // Break group and undo
        buffer.break_undo_group();
        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("".to_string()));
        assert_eq!(buffer.cursor(), Position::zero());
    }

    #[test]
    fn test_undo_redo_roundtrip() {
        let mut buffer = Buffer::new();
        buffer.insert_char('h');
        buffer.insert_char('i');
        buffer.break_undo_group();

        assert_eq!(buffer.get_line(0), Some("hi".to_string()));

        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("".to_string()));

        buffer.redo();
        assert_eq!(buffer.get_line(0), Some("hi".to_string()));
    }

    #[test]
    fn test_undo_groups_separate_on_break() {
        let mut buffer = Buffer::new();
        // Group 1: type "ab"
        buffer.insert_char('a');
        buffer.insert_char('b');
        buffer.break_undo_group();

        // Group 2: type "cd"
        buffer.insert_char('c');
        buffer.insert_char('d');
        buffer.break_undo_group();

        assert_eq!(buffer.get_line(0), Some("abcd".to_string()));

        // Undo group 2
        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("ab".to_string()));

        // Undo group 1
        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("".to_string()));
    }

    #[test]
    fn test_undo_delete() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.insert_char('b');
        buffer.insert_char('c');
        buffer.break_undo_group();

        // Delete 'c'
        buffer.delete_char_before_cursor();
        buffer.break_undo_group();
        assert_eq!(buffer.get_line(0), Some("ab".to_string()));

        // Undo the delete
        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("abc".to_string()));
    }

    #[test]
    fn test_new_edit_clears_redo() {
        let mut buffer = Buffer::new();
        buffer.insert_char('a');
        buffer.break_undo_group();

        buffer.undo();
        assert_eq!(buffer.get_line(0), Some("".to_string()));

        // New edit should clear redo
        buffer.insert_char('x');
        buffer.break_undo_group();

        // Redo should return None (stack cleared)
        assert!(buffer.redo().is_none());
        assert_eq!(buffer.get_line(0), Some("x".to_string()));
    }
}
