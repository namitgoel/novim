//! Color constants, theme structs, and color conversion utilities.

use glyphon::{Attrs, Color, Family};
use novim_core::config;

// ── Colors (loaded from theme config) ─────────────────────────────────────────

/// Default foreground — used as fallback throughout the renderer.
pub(super) const FG: Color = Color::rgb(220, 220, 220);
pub(super) const DIM: Color = Color::rgb(120, 120, 120);

// Legacy constants — still referenced by rendering code not yet migrated to GuiTheme.
#[allow(dead_code)]
pub(super) const YELLOW: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
pub(super) const RED: Color = Color::rgb(224, 108, 117);
#[allow(dead_code)]
pub(super) const BLUE: Color = Color::rgb(97, 175, 239);
#[allow(dead_code)]
pub(super) const GREEN: Color = Color::rgb(152, 195, 121);
#[allow(dead_code)]
pub(super) const TILDE_BLUE: Color = Color::rgb(65, 105, 225);
#[allow(dead_code)]
pub(super) const CURSOR_BG: Color = Color::rgb(200, 200, 200);
#[allow(dead_code)]
pub(super) const CURSOR_FG: Color = Color::rgb(30, 30, 30);
#[allow(dead_code)]
pub(super) const SELECTION_BG: Color = Color::rgb(60, 80, 120);
#[allow(dead_code)]
pub(super) const LINE_NUM: Color = Color::rgb(100, 100, 100);
#[allow(dead_code)]
pub(super) const LINE_NUM_ACTIVE: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
pub(super) const SEARCH_MATCH_BG: Color = Color::rgb(100, 80, 40);
#[allow(dead_code)]
pub(super) const EXPLORER_DIR: Color = Color::rgb(97, 175, 239);
#[allow(dead_code)]
pub(super) const EXPLORER_CURSOR_BG: Color = Color::rgb(60, 60, 70);
#[allow(dead_code)]
pub(super) const DIAG_ERROR_FG: Color = Color::rgb(224, 108, 117);
#[allow(dead_code)]
pub(super) const DIAG_WARN_FG: Color = Color::rgb(229, 192, 123);
#[allow(dead_code)]
pub(super) const BG_STATUS: Color = Color::rgb(60, 60, 66);
#[allow(dead_code)]
pub(super) const BG_TAB_ACTIVE: Color = Color::rgb(80, 80, 100);
#[allow(dead_code)]
pub(super) const BG_TAB_INACTIVE: Color = Color::rgb(40, 40, 46);
#[allow(dead_code)]
pub(super) const POPUP_BG: Color = Color::rgb(45, 45, 55);
#[allow(dead_code)]
pub(super) const POPUP_SELECTED_BG: Color = Color::rgb(70, 70, 90);
#[allow(dead_code)]
pub(super) const COMPLETION_BG: Color = Color::rgb(40, 40, 50);

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

/// Load a glyphon Color from a theme config string.
#[allow(dead_code)]
pub(super) fn theme_color(s: &str) -> Color {
    config_color_to_glyphon(config::parse_color(s))
}

/// Load a [f32;4] RGBA from a theme config string (for bg_rects).
#[allow(dead_code)]
pub(super) fn theme_color_f32(s: &str) -> [f32; 4] {
    let Color(rgba) = theme_color(s);
    let bytes = rgba.to_be_bytes();
    [bytes[0] as f32 / 255.0, bytes[1] as f32 / 255.0, bytes[2] as f32 / 255.0, 1.0]
}

/// Cached theme colors for the GUI renderer, derived from config.
#[allow(dead_code)]
pub(super) struct GuiTheme {
    pub(super) fg: Color,
    pub(super) cursor_bg: Color,
    pub(super) cursor_fg: Color,
    pub(super) selection_bg: Color,
    pub(super) search_match: Color,
    pub(super) line_num: Color,
    pub(super) line_num_active: Color,
    pub(super) tilde: Color,
    pub(super) focused_border: Color,
    pub(super) unfocused_border: Color,
    pub(super) status_bar_bg: Color,
    pub(super) mode_normal: Color,
    pub(super) mode_insert: Color,
    pub(super) mode_visual: Color,
    pub(super) mode_command: Color,
    pub(super) explorer_dir: Color,
    pub(super) diag_error: Color,
    pub(super) diag_warning: Color,
    pub(super) tab_active_bg: Color,
    pub(super) tab_inactive_bg: Color,
    pub(super) popup_bg: Color,
    pub(super) popup_border: Color,
    pub(super) popup_selected_bg: Color,
    pub(super) popup_bg_f32: [f32; 4],
    pub(super) popup_selected_f32: [f32; 4],
    pub(super) git_added: Color,
    pub(super) git_modified: Color,
    pub(super) git_deleted: Color,
}

impl GuiTheme {
    #[allow(dead_code)]
    pub(super) fn from_config(theme: &config::ThemeConfig) -> Self {
        Self {
            fg: theme_color(&theme.foreground),
            cursor_bg: theme_color(&theme.cursor_bg),
            cursor_fg: theme_color(&theme.cursor_fg),
            selection_bg: theme_color(&theme.selection_bg),
            search_match: theme_color(&theme.search_match),
            line_num: theme_color(&theme.line_number),
            line_num_active: theme_color(&theme.current_line_number),
            tilde: theme_color(&theme.tilde),
            focused_border: theme_color(&theme.focused_border),
            unfocused_border: theme_color(&theme.unfocused_border),
            status_bar_bg: theme_color(&theme.status_bar_bg),
            mode_normal: theme_color(&theme.mode_normal),
            mode_insert: theme_color(&theme.mode_insert),
            mode_visual: theme_color(&theme.mode_visual),
            mode_command: theme_color(&theme.mode_command),
            explorer_dir: theme_color(&theme.explorer_dir),
            diag_error: theme_color(&theme.diag_error),
            diag_warning: theme_color(&theme.diag_warning),
            tab_active_bg: theme_color(&theme.tab_active_bg),
            tab_inactive_bg: theme_color(&theme.tab_inactive_bg),
            popup_bg: theme_color(&theme.popup_bg),
            popup_border: theme_color(&theme.popup_border),
            popup_selected_bg: theme_color(&theme.popup_selected_bg),
            popup_bg_f32: theme_color_f32(&theme.popup_bg),
            popup_selected_f32: theme_color_f32(&theme.popup_selected_bg),
            git_added: theme_color(&theme.git_added),
            git_modified: theme_color(&theme.git_modified),
            git_deleted: theme_color(&theme.git_deleted),
        }
    }
}

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
