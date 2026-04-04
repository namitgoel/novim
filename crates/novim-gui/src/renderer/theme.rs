//! Color constants, theme structs, and color conversion utilities.

use glyphon::{Attrs, Color, Family};
use novim_core::config;

// ── Colors (loaded from theme config) ─────────────────────────────────────────

/// Default foreground — used as fallback throughout the renderer.
pub(super) const FG: Color = Color::rgb(220, 220, 220);
pub(super) const DIM: Color = Color::rgb(120, 120, 120);

// Named color constants used by the GUI renderer.
pub(super) const YELLOW: Color = Color::rgb(229, 192, 123);
pub(super) const RED: Color = Color::rgb(224, 108, 117);
pub(super) const BLUE: Color = Color::rgb(97, 175, 239);
pub(super) const GREEN: Color = Color::rgb(152, 195, 121);
pub(super) const TILDE_BLUE: Color = Color::rgb(65, 105, 225);
pub(super) const CURSOR_BG: Color = Color::rgb(200, 200, 200);
pub(super) const LINE_NUM: Color = Color::rgb(100, 100, 100);
pub(super) const LINE_NUM_ACTIVE: Color = Color::rgb(229, 192, 123);
pub(super) const EXPLORER_DIR: Color = Color::rgb(97, 175, 239);
pub(super) const DIAG_ERROR_FG: Color = Color::rgb(224, 108, 117);
pub(super) const DIAG_WARN_FG: Color = Color::rgb(229, 192, 123);

/// Tab bar accent palette.
pub(super) const TAB_ACCENTS: &[Color] = &[
    Color::rgb(97, 175, 239),  // blue
    Color::rgb(152, 195, 121), // green
    Color::rgb(198, 120, 221), // purple
    Color::rgb(224, 108, 117), // red
    Color::rgb(229, 192, 123), // gold
    Color::rgb(86, 182, 194),  // teal
];

// ── Span builder utility ──────────────────────────────────────────────────────

/// A colored text span for rich text rendering.
pub(super) struct RichSpan {
    pub(super) text: String,
    pub(super) color: Color,
}

// RichSpan Clone impl
impl Clone for RichSpan {
    fn clone(&self) -> Self {
        RichSpan { text: self.text.clone(), color: self.color }
    }
}

pub(super) fn mono_attrs(color: Color) -> Attrs<'static> {
    Attrs::new().family(Family::Monospace).color(color)
}

// ── Theme config ─────────────────────────────────────────────────────────────

// ── Color conversion ─────────────────────────────────────────────────────────

pub(super) fn config_color_to_glyphon(c: config::Color) -> Color {
    match c {
        config::Color::Black => Color::rgb(0, 0, 0),
        config::Color::Red => Color::rgb(224, 108, 117),
        config::Color::Green => Color::rgb(152, 195, 121),
        config::Color::Yellow => Color::rgb(229, 192, 123),
        config::Color::Blue => Color::rgb(97, 175, 239),
        config::Color::Magenta => Color::rgb(198, 120, 221),
        config::Color::Cyan => Color::rgb(86, 182, 194),
        config::Color::White => Color::rgb(220, 220, 220),
        config::Color::DarkGray => Color::rgb(100, 100, 100),
        config::Color::LightRed => Color::rgb(240, 140, 140),
        config::Color::LightGreen => Color::rgb(180, 220, 160),
        config::Color::LightYellow => Color::rgb(240, 210, 150),
        config::Color::LightBlue => Color::rgb(140, 200, 250),
        config::Color::LightMagenta => Color::rgb(220, 160, 240),
        config::Color::LightCyan => Color::rgb(130, 210, 220),
        config::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
        config::Color::Indexed(idx) => indexed_256_to_rgb(idx),
    }
}

/// Convert a 256-color index to approximate RGB.
pub(super) fn indexed_256_to_rgb(idx: u8) -> Color {
    match idx {
        0 => Color::rgb(0, 0, 0),
        1 => Color::rgb(128, 0, 0),
        2 => Color::rgb(0, 128, 0),
        3 => Color::rgb(128, 128, 0),
        4 => Color::rgb(0, 0, 128),
        5 => Color::rgb(128, 0, 128),
        6 => Color::rgb(0, 128, 128),
        7 => Color::rgb(192, 192, 192),
        8 => Color::rgb(128, 128, 128),
        9 => Color::rgb(255, 0, 0),
        10 => Color::rgb(0, 255, 0),
        11 => Color::rgb(255, 255, 0),
        12 => Color::rgb(0, 0, 255),
        13 => Color::rgb(255, 0, 255),
        14 => Color::rgb(0, 255, 255),
        15 => Color::rgb(255, 255, 255),
        16..=231 => {
            let n = idx - 16;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            Color::rgb(r, g, b)
        }
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            Color::rgb(gray, gray, gray)
        }
    }
}
