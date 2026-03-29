//! Shared types for Novim
//!
//! This crate contains type definitions shared between Rust modules
//! and serializable types for communication with TypeScript.

use serde::{Deserialize, Serialize};

/// Position in a text buffer (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    pub fn zero() -> Self {
        Self { line: 0, column: 0 }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::zero()
    }
}

/// Editor mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    Command,
}

impl EditorMode {
    /// Get the display name for the mode (e.g., for status bar)
    pub fn display_name(&self) -> &'static str {
        match self {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::Command => "COMMAND",
        }
    }

    /// Get the short name for compact display
    pub fn short_name(&self) -> &'static str {
        match self {
            EditorMode::Normal => "N",
            EditorMode::Insert => "I",
            EditorMode::Visual => "V",
            EditorMode::Command => "C",
        }
    }
}

impl Default for EditorMode {
    fn default() -> Self {
        EditorMode::Normal
    }
}

/// Direction for navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// A text selection (anchor = where selection started, head = where cursor is now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub head: Position,
}

impl Selection {
    pub fn new(anchor: Position, head: Position) -> Self {
        Self { anchor, head }
    }

    /// Get the start and end positions in document order (start <= end).
    pub fn ordered(&self) -> (Position, Position) {
        if self.anchor.line < self.head.line
            || (self.anchor.line == self.head.line && self.anchor.column <= self.head.column)
        {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Check if a position is within this selection.
    pub fn contains(&self, pos: Position) -> bool {
        let (start, end) = self.ordered();
        if pos.line < start.line || pos.line > end.line {
            return false;
        }
        if pos.line == start.line && pos.column < start.column {
            return false;
        }
        if pos.line == end.line && pos.column > end.column {
            return false;
        }
        true
    }
}

/// A rendering-agnostic rectangle (replaces ratatui::layout::Rect in core code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self { x, y, width, height }
    }
}
