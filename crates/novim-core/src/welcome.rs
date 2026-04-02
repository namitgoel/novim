//! Welcome screen content — ASCII logo + shortcut hints.

/// A single line in the welcome screen, with optional highlight color.
pub struct WelcomeLine {
    pub text: String,
    /// "logo", "version", "shortcut_key", "shortcut_desc", or "blank"
    pub kind: &'static str,
}

/// Build the welcome screen lines, centered for the given width.
pub fn welcome_lines() -> Vec<WelcomeLine> {
    let version = env!("CARGO_PKG_VERSION");

    let logo = [
        r"  ███╗   ██╗██╗   ██╗  ",
        r"  ████╗  ██║██║   ██║  ",
        r"  ██╔██╗ ██║██║   ██║  ",
        r"  ██║╚██╗██║╚██╗ ██╔╝  ",
        r"  ██║ ╚████║ ╚████╔╝   ",
        r"  ╚═╝  ╚═══╝  ╚═══╝    ",
    ];

    let mut lines: Vec<WelcomeLine> = Vec::new();

    // Logo
    for l in &logo {
        lines.push(WelcomeLine { text: l.to_string(), kind: "logo" });
    }

    // Blank + version
    lines.push(WelcomeLine { text: String::new(), kind: "blank" });
    lines.push(WelcomeLine {
        text: format!("novim v{}", version),
        kind: "version",
    });
    lines.push(WelcomeLine {
        text: "Hybrid terminal multiplexer & modal text editor".to_string(),
        kind: "version",
    });

    // Blank + shortcuts
    lines.push(WelcomeLine { text: String::new(), kind: "blank" });

    let shortcuts = [
        ("e", "New file"),
        ("t", "Open terminal"),
        ("f", "Find file"),
        ("?", "Help"),
        ("q", "Quit"),
    ];

    for (key, desc) in &shortcuts {
        lines.push(WelcomeLine {
            text: format!("  {}   {}", key, desc),
            kind: "shortcut",
        });
    }

    lines
}
