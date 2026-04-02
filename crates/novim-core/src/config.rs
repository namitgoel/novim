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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Border color for focused pane: "cyan", "green", "yellow", etc.
    pub focused_border: String,
    /// Border color for unfocused panes
    pub unfocused_border: String,
    /// Status bar background
    pub status_bar_bg: String,
    /// Line number color
    pub line_number: String,
    /// Current line number color
    pub current_line_number: String,
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
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            focused_border: "cyan".to_string(),
            unfocused_border: "darkgray".to_string(),
            status_bar_bg: "darkgray".to_string(),
            line_number: "darkgray".to_string(),
            current_line_number: "yellow".to_string(),
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
pub fn load_config() -> NovimConfig {
    let Some(path) = config_path() else {
        return NovimConfig::default();
    };

    match fs::read_to_string(&path) {
        Ok(content) => {
            toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("Warning: config parse error: {}. Using defaults.", e);
                NovimConfig::default()
            })
        }
        Err(_) => NovimConfig::default(),
    }
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
