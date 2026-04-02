//! Novim CLI — standalone terminal binary.
//!
//! Provides the same functionality as the TypeScript CLI
//! but as a single native binary with no Node.js dependency.

use std::io::{self, Write};
use std::process;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use novim_core::config;
use novim_core::session;
use novim_tui::TerminalManager;

#[derive(Parser)]
#[command(
    name = "novim",
    version,
    about = "A hybrid terminal multiplexer and modal text editor"
)]
struct Cli {
    /// File to open
    file: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Attach to a saved session
    Attach {
        /// Session name
        name: String,
    },
    /// List saved sessions
    List,
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Open config file in novim
    Edit,
    /// Print the config file path
    Path,
    /// Reset config to defaults
    Reset,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Some(Commands::Attach { name }) => run_attach(&name),
        Some(Commands::List) => run_list(),
        Some(Commands::Config { action }) => run_config(action),
        Some(Commands::Completions { shell }) => run_completions(shell),
        None => match cli.file {
            Some(ref path) => run_with_file(path),
            None => run_terminal(),
        },
    };

    if let Err(e) = result {
        eprintln!("novim: {}", e);
        process::exit(1);
    }
}

/// Open with a shell terminal pane (tmux-like mode).
fn run_terminal() -> io::Result<()> {
    let mut tm = TerminalManager::with_terminal()?;
    tm.run()?;
    tm.shutdown()
}

/// Open a file in the editor.
fn run_with_file(path: &str) -> io::Result<()> {
    let mut tm = TerminalManager::with_file(path)?;
    tm.run()?;
    tm.shutdown()
}

/// Restore a saved session.
fn run_attach(name: &str) -> io::Result<()> {
    let mut tm = TerminalManager::with_session(name)?;
    tm.run()?;
    tm.shutdown()
}

/// List saved sessions.
fn run_list() -> io::Result<()> {
    let sessions = session::list_sessions().map_err(|e| {
        io::Error::other(e.to_string())
    })?;

    if sessions.is_empty() {
        println!("No saved sessions.");
        println!("Save a session with Ctrl+W m inside novim.");
    } else {
        println!("Saved sessions:");
        for name in &sessions {
            println!("  {}", name);
        }
        println!();
        println!("Attach with: novim attach <name>");
    }
    Ok(())
}

/// Handle config subcommands.
fn run_config(action: ConfigAction) -> io::Result<()> {
    match action {
        ConfigAction::Path => {
            match config::config_path() {
                Some(path) => println!("{}", path.display()),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine config path (HOME not set)",
                    ));
                }
            }
            Ok(())
        }
        ConfigAction::Edit => {
            let path = config::config_path().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Could not determine config path (HOME not set)")
            })?;

            // Create default config if it doesn't exist
            if !path.exists() {
                config::save_default_config()?;
            }

            let path_str = path.to_string_lossy().to_string();
            let mut tm = TerminalManager::with_file(&path_str)?;
            tm.run()?;
            tm.shutdown()
        }
        ConfigAction::Reset => {
            let msg = config::save_default_config()?;
            println!("{}", msg);
            Ok(())
        }
    }
}

/// Generate shell completions to stdout.
fn run_completions(shell: Shell) -> io::Result<()> {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "novim", &mut io::stdout());
    io::stdout().flush()
}
