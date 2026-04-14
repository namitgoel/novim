//! Configuration system — loads from ~/.config/novim/config.toml

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Novim configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NovimConfig {
    pub editor: EditorConfig,
    pub theme: ThemeConfig,
    pub syntax_theme: SyntaxTheme,
    pub terminal: TerminalConfig,
    pub lsp: LspConfig,
    pub keybindings: KeybindingsConfig,
    pub gui: GuiConfig,
    pub status_bar: StatusBarConfig,
    /// Active color scheme name (persisted across sessions).
    #[serde(default)]
    pub colorscheme: Option<String>,
}

/// GUI-specific configuration (font, window settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    /// Font family name (e.g., "JetBrains Mono", "Fira Code", "monospace")
    pub font_family: String,
    /// Base font size in points (scaled by DPI)
    pub font_size: f32,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            font_family: "monospace".to_string(),
            font_size: 14.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Tab width in spaces
    pub tab_width: usize,
    /// Insert spaces instead of tab characters
    pub expand_tab: bool,
    /// Copy indentation from current line on Enter
    pub auto_indent: bool,
    /// Line number mode: "absolute", "relative", "hybrid", "off"
    pub line_numbers: String,
    /// Show status bar
    pub status_bar: bool,
    /// Scroll offset (lines to keep visible above/below cursor)
    pub scroll_offset: usize,
    /// Show file preview in finder (true = split view, false = list only)
    pub finder_preview: bool,
    /// Enable word wrapping
    pub word_wrap: bool,
    /// Lines to scroll with Ctrl+U/D
    pub scroll_lines: usize,
    /// Lines to scroll per mouse wheel tick
    pub mouse_scroll_lines: usize,
    /// Text width for `gq` formatting (0 = use 80)
    pub text_width: usize,
    /// Show minimap code overview on the right side of editor panes.
    pub minimap: bool,
    /// Minimap width in columns (default 8).
    pub minimap_width: usize,
}

/// Status bar display configuration.
/// Format placeholders: {mode}, {file}, {modified}, {line}, {col}, {total},
/// {percent}, {lsp}, {branch}, {diag}, {pane}, {message}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Format string for the left side of the status bar.
    pub left: String,
    /// Format string for the right side of the status bar.
    pub right: String,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            left: " {mode} | {message}{diag}{pane}".to_string(),
            right: "{lsp}{branch} | {line}:{col} | {line}/{total} ".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    // ── Pane borders ──
    pub focused_border: String,
    pub unfocused_border: String,
    // ── General ──
    pub foreground: String,
    pub background: String,
    // ── Cursor ──
    pub cursor_bg: String,
    pub cursor_fg: String,
    // ── Selection & search ──
    pub selection_bg: String,
    pub search_match: String,
    // ── Gutter ──
    pub line_number: String,
    pub current_line_number: String,
    pub tilde: String,
    // ── Status bar ──
    pub status_bar_bg: String,
    pub mode_normal: String,
    pub mode_insert: String,
    pub mode_visual: String,
    pub mode_command: String,
    // ── Explorer ──
    pub explorer_dir: String,
    // ── Diagnostics ──
    pub diag_error: String,
    pub diag_warning: String,
    // ── Tabs ──
    pub tab_active_bg: String,
    pub tab_inactive_bg: String,
    // ── Popups ──
    pub popup_bg: String,
    pub popup_border: String,
    pub popup_selected_bg: String,
    // ── Git gutter ──
    pub git_added: String,
    pub git_modified: String,
    pub git_deleted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Shell to spawn in terminal panes (default: $SHELL or /bin/sh)
    pub shell: Option<String>,
    /// TERM environment variable
    pub term_env: String,
}

/// Syntax highlight color theme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyntaxTheme {
    pub keyword: String,
    pub function: String,
    pub r#type: String,
    pub variable: String,
    pub string: String,
    pub number: String,
    pub comment: String,
    pub operator: String,
    pub punctuation: String,
    pub property: String,
    pub constant: String,
    pub attribute: String,
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        Self {
            keyword: "#af87d7".to_string(),     // soft purple (indexed 176)
            function: "#5fafff".to_string(),     // soft blue (indexed 75)
            r#type: "#d7af87".to_string(),       // soft gold (indexed 180)
            variable: "#d0d0d0".to_string(),     // light gray (indexed 252)
            string: "#87af87".to_string(),       // muted green (indexed 108)
            number: "#87d7ff".to_string(),       // soft teal (indexed 117)
            comment: "#767676".to_string(),      // medium gray (indexed 243)
            operator: "#bcbcbc".to_string(),     // light gray (indexed 250)
            punctuation: "#8a8a8a".to_string(),  // gray (indexed 245)
            property: "#87afd7".to_string(),     // muted blue (indexed 110)
            constant: "#87d7ff".to_string(),     // soft teal (indexed 117)
            attribute: "#d7af87".to_string(),    // soft gold (indexed 180)
        }
    }
}

/// Keybinding configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    /// Normal mode keybindings: { "u" = "undo", "Ctrl+z" = "undo" }
    pub normal: HashMap<String, String>,
    /// Insert mode keybindings
    pub insert: HashMap<String, String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LspConfig {
    /// Enable LSP support
    pub enabled: bool,
    /// Per-language server configurations (config overrides built-in providers)
    pub servers: HashMap<String, LspServerConfigEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfigEntry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: HashMap::new(),
        }
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_width: 4,
            expand_tab: true,
            auto_indent: true,
            line_numbers: "hybrid".to_string(),
            status_bar: true,
            scroll_offset: 3,
            finder_preview: true,
            word_wrap: false,
            scroll_lines: 10,
            mouse_scroll_lines: 3,
            text_width: 80,
            minimap: false,
            minimap_width: 8,
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            focused_border: "cyan".to_string(),
            unfocused_border: "darkgray".to_string(),
            foreground: "#dcdcdc".to_string(),
            background: "#1e1e1e".to_string(),
            cursor_bg: "#c8c8c8".to_string(),
            cursor_fg: "#1e1e1e".to_string(),
            selection_bg: "#3c5078".to_string(),
            search_match: "#645028".to_string(),
            line_number: "darkgray".to_string(),
            current_line_number: "yellow".to_string(),
            tilde: "#4169e1".to_string(),
            status_bar_bg: "darkgray".to_string(),
            mode_normal: "#61afef".to_string(),
            mode_insert: "#98c379".to_string(),
            mode_visual: "#e5c07b".to_string(),
            mode_command: "#e5c07b".to_string(),
            explorer_dir: "#61afef".to_string(),
            diag_error: "#e06c75".to_string(),
            diag_warning: "#e5c07b".to_string(),
            tab_active_bg: "#505064".to_string(),
            tab_inactive_bg: "#28282e".to_string(),
            popup_bg: "#2d2d37".to_string(),
            popup_border: "#50b4e6".to_string(),
            popup_selected_bg: "#46465a".to_string(),
            git_added: "#98c379".to_string(),
            git_modified: "#e5c07b".to_string(),
            git_deleted: "#e06c75".to_string(),
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: None,
            term_env: "xterm-256color".to_string(),
        }
    }
}

/// Get the config file path (~/.config/novim/config.toml).
pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config").join("novim").join("config.toml"))
}

/// Load configuration from disk, or return defaults.
/// If a colorscheme is set, applies it on top of the config.
pub fn load_config() -> NovimConfig {
    let Some(path) = config_path() else {
        return NovimConfig::default();
    };

    let mut config = match fs::read_to_string(&path) {
        Ok(content) => {
            toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("Warning: config parse error: {}. Using defaults.", e);
                NovimConfig::default()
            })
        }
        Err(_) => NovimConfig::default(),
    };

    // Apply colorscheme if set
    if let Some(ref name) = config.colorscheme {
        if let Some(scheme) = crate::theme::load_theme(name) {
            config.theme = scheme.to_theme_config();
            config.syntax_theme = scheme.to_syntax_theme();
        }
    }

    config
}

/// Save the default config as a template.
pub fn save_default_config() -> std::io::Result<String> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let config = NovimConfig::default();
    let content = toml::to_string_pretty(&config)
        .map_err(std::io::Error::other)?;

    fs::write(&path, &content)?;
    Ok(format!("Config saved to {}", path.display()))
}

/// Parse a color name string to a usable color identifier.
/// Parse a color string. Supports:
/// - Named colors: "red", "cyan", "darkgray"
/// - Hex colors: "#c586c0", "#ff0000"
/// - 256-color index: "176", "75"
pub fn parse_color(name: &str) -> Color {
    let name = name.trim();

    // Hex color: #rrggbb
    if let Some(hex) = name.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }

    // Numeric: 256-color index
    if let Ok(idx) = name.parse::<u8>() {
        return Color::Indexed(idx);
    }

    // Named colors
    match name.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "darkgray" | "dark_gray" | "gray" => Color::DarkGray,
        "lightred" | "light_red" => Color::LightRed,
        "lightgreen" | "light_green" => Color::LightGreen,
        "lightyellow" | "light_yellow" => Color::LightYellow,
        "lightblue" | "light_blue" => Color::LightBlue,
        "lightmagenta" | "light_magenta" => Color::LightMagenta,
        "lightcyan" | "light_cyan" => Color::LightCyan,
        _ => Color::White,
    }
}

/// Color enum — supports named, indexed, and RGB.
#[derive(Debug, Clone, Copy)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    Rgb(u8, u8, u8),
    Indexed(u8),
}
