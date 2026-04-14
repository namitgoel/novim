//! Tab completion providers for `:` command mode.

use crate::input::known_commands;

/// All known `:set` option names.
const OPTIONS: &[&str] = &[
    "rnu", "relativenumber",
    "nonu", "nonumber",
    "number", "nu",
    "wrap", "nowrap",
    "et", "expandtab", "noet", "noexpandtab",
    "ai", "autoindent", "noai", "noautoindent",
    "minimap", "nominimap",
    "all",
];

/// Complete an ex-command name from a prefix.
/// Includes built-in commands and any plugin-registered commands.
pub fn complete_command(prefix: &str, plugin_commands: &[String]) -> Vec<String> {
    let mut matches: Vec<String> = known_commands().iter()
        .filter(|c| c.starts_with(prefix) && **c != prefix)
        .map(|c| c.to_string())
        .collect();
    // Add plugin-registered commands
    for cmd in plugin_commands {
        if cmd.starts_with(prefix) && cmd != prefix {
            matches.push(cmd.clone());
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

/// Complete an option name from a prefix (after `:set `).
pub fn complete_option(prefix: &str) -> Vec<String> {
    let mut matches: Vec<String> = OPTIONS.iter()
        .filter(|o| o.starts_with(prefix) && **o != prefix)
        .map(|o| o.to_string())
        .collect();
    // Also complete ts=N style
    if "ts".starts_with(prefix) || prefix.starts_with("ts") {
        if !matches.contains(&"ts=".to_string()) {
            matches.push("ts=".to_string());
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

/// Complete a file path from a partial path.
pub fn complete_filepath(partial: &str) -> Vec<String> {
    let (dir_part, file_prefix) = if let Some(pos) = partial.rfind('/') {
        (&partial[..=pos], &partial[pos + 1..])
    } else {
        ("", partial)
    };

    let search_dir = if dir_part.is_empty() {
        ".".to_string()
    } else if dir_part.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/{}", home, &dir_part[2..])
    } else {
        dir_part.to_string()
    };

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };

    let mut matches: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(file_prefix) {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let full = format!("{}{}{}", dir_part, name, if is_dir { "/" } else { "" });
                Some(full)
            } else {
                None
            }
        })
        .collect();
    matches.sort();
    matches
}

/// Determine the completion context from the current command buffer.
/// Returns a list of completion candidates.
/// `plugin_commands` is the list of plugin-registered command names.
pub fn complete_command_buffer(buffer: &str, plugin_commands: &[String]) -> Vec<String> {
    let trimmed = buffer.trim_start();

    // If no space yet, complete the command name
    if !trimmed.contains(' ') {
        return complete_command(trimmed, plugin_commands);
    }

    // Split into command and argument
    let (cmd, args) = match trimmed.split_once(' ') {
        Some((c, a)) => (c, a),
        None => return Vec::new(),
    };

    match cmd {
        // Commands that take file paths
        "e" | "edit" | "read" | "r" | "source" | "so" | "tabnew" | "tabe"
        | "explore" | "Explore" | "Ex" | "cd" | "lcd" | "chdir" => {
            // Don't complete after :read !cmd
            if cmd == "read" || cmd == "r" {
                if args.starts_with('!') {
                    return Vec::new();
                }
            }
            complete_filepath(args)
        }
        // :set — complete option names
        "set" => complete_option(args),
        // :colorscheme — complete theme names
        "colorscheme" | "colo" => {
            let themes = crate::theme::available_themes();
            themes.into_iter()
                .filter(|t| t.starts_with(args) && t != args)
                .collect()
        }
        _ => Vec::new(),
    }
}
