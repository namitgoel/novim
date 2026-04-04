//! Novim Core — rendering-agnostic editor engine.
//!
//! Contains: text buffers, pane management, terminal emulation,
//! session persistence, input/command system, error types.
//!
//! This crate has NO dependency on Ratatui or Neon.
//! Rendering is handled by novim-tui (or a future novim-gui).
//! FFI is handled by novim-neon.

pub mod async_task;
pub mod buffer;
pub mod config;
pub mod editor;
pub mod emulator;
pub mod explorer;
pub mod finder;
/// Re-export syntax highlighting types from the plugin module.
pub use plugin::builtins::syntax as highlight;
pub mod lsp;
pub mod error;
pub mod fold;
pub mod git;
pub mod help;
pub mod input;
pub mod pane;
pub mod session;
pub mod plugin;
pub mod text_utils;
pub mod url;
pub mod welcome;
