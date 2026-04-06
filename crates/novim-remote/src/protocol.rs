//! Protocol types for novim remote communication.
//!
//! Client sends raw input events, server sends rendered cell grids.
//! Uses Content-Length framed JSON over stdin/stdout (SSH pipe).

use serde::{Deserialize, Serialize};

/// Protocol version. Bump when breaking changes are made.
pub const PROTOCOL_VERSION: u32 = 1;

// ── Client → Server ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Initial handshake with terminal dimensions.
    Hello { version: u32, width: u16, height: u16 },
    /// Keyboard input. `code` uses crossterm key code names, `modifiers` is a bitmask.
    Key { code: String, modifiers: u8 },
    /// Mouse input.
    Mouse { kind: String, col: u16, row: u16, modifiers: u8 },
    /// Terminal was resized.
    Resize { width: u16, height: u16 },
    /// Keep-alive ping.
    Ping,
    /// Client is disconnecting gracefully.
    Disconnect,
}

// ── Server → Client ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Handshake acknowledgment.
    HelloAck { version: u32 },
    /// Full frame: entire screen as a cell grid.
    Frame { cells: Vec<Vec<StyledCell>>, cursor: Option<(u16, u16)> },
    /// Delta update: only changed cells since last frame.
    Delta { changes: Vec<(u16, u16, StyledCell)>, cursor: Option<(u16, u16)> },
    /// Keep-alive pong.
    Pong,
    /// Server is shutting down.
    Bye,
    /// Error message.
    Error { message: String },
}

/// A single styled terminal cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyledCell {
    /// Character to display.
    pub c: char,
    /// Foreground color as RGB.
    pub fg: [u8; 3],
    /// Background color as RGB.
    pub bg: [u8; 3],
    /// Attribute flags: bit 0=bold, bit 1=dim, bit 2=underline, bit 3=reverse.
    pub attrs: u8,
}

impl StyledCell {
    pub fn new(c: char, fg: [u8; 3], bg: [u8; 3], attrs: u8) -> Self {
        Self { c, fg, bg, attrs }
    }

    pub fn blank() -> Self {
        Self { c: ' ', fg: [220, 220, 220], bg: [0, 0, 0], attrs: 0 }
    }
}

/// Modifier bitmask constants (matching crossterm's KeyModifiers).
pub mod modifiers {
    pub const SHIFT: u8 = 0b0001;
    pub const CONTROL: u8 = 0b0010;
    pub const ALT: u8 = 0b0100;
}
