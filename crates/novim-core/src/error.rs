//! Unified error type for novim-core.

use std::fmt;
use std::io;

#[derive(Debug)]
pub enum NovimError {
    /// I/O errors (file, terminal, PTY)
    Io(io::Error),
    /// Session errors (save/load/parse)
    Session(String),
    /// Buffer errors (no file path, etc.)
    Buffer(String),
    /// Command errors (unknown command, bad args)
    Command(String),
}

impl fmt::Display for NovimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NovimError::Io(e) => write!(f, "{}", e),
            NovimError::Session(msg) => write!(f, "Session: {}", msg),
            NovimError::Buffer(msg) => write!(f, "{}", msg),
            NovimError::Command(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for NovimError {}

impl From<io::Error> for NovimError {
    fn from(e: io::Error) -> Self {
        NovimError::Io(e)
    }
}

impl From<serde_json::Error> for NovimError {
    fn from(e: serde_json::Error) -> Self {
        NovimError::Session(e.to_string())
    }
}

/// Convenience type alias
pub type Result<T> = std::result::Result<T, NovimError>;
