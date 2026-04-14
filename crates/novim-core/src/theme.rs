//! Theme system — load color schemes from built-in themes or user files.
//!
//! Themes are TOML files in `~/.config/novim/themes/` or built-in.
//! `:colorscheme <name>` applies a theme and persists it to config.

use std::path::PathBuf;

use crate::config::{ThemeConfig, SyntaxTheme};

/// A complete color scheme that maps to ThemeConfig + SyntaxTheme.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct ColorScheme {
    pub name: String,
    #[serde(default)]
    pub ui: UiColors,
    #[serde(default)]
    pub syntax: SyntaxColors,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct UiColors {
    pub foreground: String,
    pub background: String,
    pub cursor_bg: String,
    pub cursor_fg: String,
    pub selection_bg: String,
    pub search_match: String,
    pub line_number: String,
    pub current_line_number: String,
    pub tilde: String,
    pub focused_border: String,
    pub unfocused_border: String,
    pub status_bar_bg: String,
    pub mode_normal: String,
    pub mode_insert: String,
    pub mode_visual: String,
    pub mode_command: String,
    pub explorer_dir: String,
    pub diag_error: String,
    pub diag_warning: String,
    pub tab_active_bg: String,
    pub tab_inactive_bg: String,
    pub popup_bg: String,
    pub popup_border: String,
    pub popup_selected_bg: String,
    pub git_added: String,
    pub git_modified: String,
    pub git_deleted: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct SyntaxColors {
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

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            ui: UiColors::default(),
            syntax: SyntaxColors::default(),
        }
    }
}

impl Default for UiColors {
    fn default() -> Self {
        let t = ThemeConfig::default();
        Self {
            foreground: t.foreground, background: t.background,
            cursor_bg: t.cursor_bg, cursor_fg: t.cursor_fg,
            selection_bg: t.selection_bg, search_match: t.search_match,
            line_number: t.line_number, current_line_number: t.current_line_number,
            tilde: t.tilde, focused_border: t.focused_border,
            unfocused_border: t.unfocused_border, status_bar_bg: t.status_bar_bg,
            mode_normal: t.mode_normal, mode_insert: t.mode_insert,
            mode_visual: t.mode_visual, mode_command: t.mode_command,
            explorer_dir: t.explorer_dir, diag_error: t.diag_error,
            diag_warning: t.diag_warning, tab_active_bg: t.tab_active_bg,
            tab_inactive_bg: t.tab_inactive_bg, popup_bg: t.popup_bg,
            popup_border: t.popup_border, popup_selected_bg: t.popup_selected_bg,
            git_added: t.git_added, git_modified: t.git_modified,
            git_deleted: t.git_deleted,
        }
    }
}

impl Default for SyntaxColors {
    fn default() -> Self {
        let s = SyntaxTheme::default();
        Self {
            keyword: s.keyword, function: s.function, r#type: s.r#type,
            variable: s.variable, string: s.string, number: s.number,
            comment: s.comment, operator: s.operator, punctuation: s.punctuation,
            property: s.property, constant: s.constant, attribute: s.attribute,
        }
    }
}

impl ColorScheme {
    /// Convert to ThemeConfig.
    pub fn to_theme_config(&self) -> ThemeConfig {
        ThemeConfig {
            foreground: self.ui.foreground.clone(), background: self.ui.background.clone(),
            cursor_bg: self.ui.cursor_bg.clone(), cursor_fg: self.ui.cursor_fg.clone(),
            selection_bg: self.ui.selection_bg.clone(), search_match: self.ui.search_match.clone(),
            line_number: self.ui.line_number.clone(), current_line_number: self.ui.current_line_number.clone(),
            tilde: self.ui.tilde.clone(), focused_border: self.ui.focused_border.clone(),
            unfocused_border: self.ui.unfocused_border.clone(), status_bar_bg: self.ui.status_bar_bg.clone(),
            mode_normal: self.ui.mode_normal.clone(), mode_insert: self.ui.mode_insert.clone(),
            mode_visual: self.ui.mode_visual.clone(), mode_command: self.ui.mode_command.clone(),
            explorer_dir: self.ui.explorer_dir.clone(), diag_error: self.ui.diag_error.clone(),
            diag_warning: self.ui.diag_warning.clone(), tab_active_bg: self.ui.tab_active_bg.clone(),
            tab_inactive_bg: self.ui.tab_inactive_bg.clone(), popup_bg: self.ui.popup_bg.clone(),
            popup_border: self.ui.popup_border.clone(), popup_selected_bg: self.ui.popup_selected_bg.clone(),
            git_added: self.ui.git_added.clone(), git_modified: self.ui.git_modified.clone(),
            git_deleted: self.ui.git_deleted.clone(),
        }
    }

    /// Convert to SyntaxTheme.
    pub fn to_syntax_theme(&self) -> SyntaxTheme {
        SyntaxTheme {
            keyword: self.syntax.keyword.clone(), function: self.syntax.function.clone(),
            r#type: self.syntax.r#type.clone(), variable: self.syntax.variable.clone(),
            string: self.syntax.string.clone(), number: self.syntax.number.clone(),
            comment: self.syntax.comment.clone(), operator: self.syntax.operator.clone(),
            punctuation: self.syntax.punctuation.clone(), property: self.syntax.property.clone(),
            constant: self.syntax.constant.clone(), attribute: self.syntax.attribute.clone(),
        }
    }
}

// ── Built-in themes ──

const CATPPUCCIN_MOCHA: &str = include_str!("themes/catppuccin-mocha.toml");
const CATPPUCCIN_LATTE: &str = include_str!("themes/catppuccin-latte.toml");
const DRACULA: &str = include_str!("themes/dracula.toml");
const ONE_DARK: &str = include_str!("themes/one-dark.toml");
const SOLARIZED_DARK: &str = include_str!("themes/solarized-dark.toml");
const GOOGLE_LIGHT: &str = include_str!("themes/google-light.toml");

/// Get the list of available theme names (built-in + user).
pub fn available_themes() -> Vec<String> {
    let mut themes = vec![
        "default".to_string(),
        "catppuccin-mocha".to_string(),
        "catppuccin-latte".to_string(),
        "dracula".to_string(),
        "one-dark".to_string(),
        "solarized-dark".to_string(),
        "google-light".to_string(),
    ];

    // Add user themes from ~/.config/novim/themes/
    if let Some(dir) = themes_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    let name = name.to_string();
                    if !themes.contains(&name) {
                        themes.push(name);
                    }
                }
            }
        }
    }

    themes
}

/// Load a theme by name. Checks built-in first, then user themes directory.
pub fn load_theme(name: &str) -> Option<ColorScheme> {
    // Built-in themes
    let builtin = match name {
        "default" => return Some(ColorScheme::default()),
        "catppuccin-mocha" => Some(CATPPUCCIN_MOCHA),
        "catppuccin-latte" => Some(CATPPUCCIN_LATTE),
        "dracula" => Some(DRACULA),
        "one-dark" => Some(ONE_DARK),
        "solarized-dark" => Some(SOLARIZED_DARK),
        "google-light" => Some(GOOGLE_LIGHT),
        _ => None,
    };

    if let Some(toml_str) = builtin {
        return toml::from_str(toml_str).ok();
    }

    // User theme from ~/.config/novim/themes/<name>.toml
    let dir = themes_dir()?;
    let path = dir.join(format!("{}.toml", name));
    let content = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&content).ok()
}

fn themes_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config").join("novim").join("themes"))
}
