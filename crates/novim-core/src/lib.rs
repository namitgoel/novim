//! Novim Core — rendering-agnostic editor engine.
//!
//! Contains: text buffers, pane management, terminal emulation,
//! session persistence, input/command system, error types.
//!
//! This crate has NO dependency on Ratatui or Neon.
//! Rendering is handled by novim-tui (or a future novim-gui).
//! FFI is handled by novim-neon.

pub mod buffer;
pub mod config;
pub mod editor;
pub mod emulator;
pub mod explorer;
pub mod finder;
pub mod highlight;
pub mod lsp;
pub mod error;
pub mod fold;
pub mod input;
pub mod git;
pub mod pane;
pub mod session;
pub mod url;
pub mod welcome;
