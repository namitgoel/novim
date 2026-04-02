//! URL detection — find URLs in text for highlighting and click-to-open.

use regex::Regex;
use std::sync::OnceLock;

/// A detected URL with its position in the text.
#[derive(Debug, Clone)]
pub struct UrlMatch {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https?://[^\s)<>\]]+").unwrap()
    })
}

/// Find all URLs in a line of text.
pub fn find_urls(text: &str) -> Vec<UrlMatch> {
    url_regex()
        .find_iter(text)
        .map(|m| UrlMatch {
            start: m.start(),
            end: m.end(),
            url: m.as_str().to_string(),
        })
        .collect()
}

/// Check if a specific column position falls on a URL. Returns the URL if found.
pub fn url_at_position(text: &str, col: usize) -> Option<String> {
    for m in find_urls(text) {
        if col >= m.start && col < m.end {
            return Some(m.url);
        }
    }
    None
}

/// Open a URL in the default browser (macOS: `open`, Linux: `xdg-open`).
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", url]).spawn();
}
