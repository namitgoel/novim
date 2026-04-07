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
    /// File or directory to open
    path: Option<String>,

    /// Enable debug logging to ~/.novim/debug.log
    #[arg(long)]
    debug: bool,

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

    if cli.debug {
        init_debug_logger();
    }
    log::debug!("novim starting, args: path={:?}", cli.path);

    let result = match cli.command {
        Some(Commands::Attach { name }) => run_attach(&name),
        Some(Commands::List) => run_list(),
        Some(Commands::Config { action }) => run_config(action),
        Some(Commands::Completions { shell }) => run_completions(shell),
        None => match cli.path {
            Some(ref path) => {
                let p = std::path::Path::new(path);
                if p.is_dir() {
                    run_with_dir(path)
                } else {
                    run_with_file(path)
                }
            }
            None => run_welcome(),
        },
    };

    if let Err(e) = result {
        eprintln!("novim: {}", e);
        process::exit(1);
    }
}

/// Run a TerminalManager to completion: run the event loop then shut down.
fn run_terminal(mut tm: TerminalManager) -> io::Result<()> {
    tm.run()?;
    tm.shutdown()
}

/// Open the welcome screen (no args).
fn run_welcome() -> io::Result<()> {
    run_terminal(TerminalManager::with_welcome()?)
}

/// Open a directory with explorer sidebar.
fn run_with_dir(path: &str) -> io::Result<()> {
    run_terminal(TerminalManager::with_dir(path)?)
}

/// Open a file in the editor.
fn run_with_file(path: &str) -> io::Result<()> {
    run_terminal(TerminalManager::with_file(path)?)
}

/// Restore a saved session.
fn run_attach(name: &str) -> io::Result<()> {
    run_terminal(TerminalManager::with_session(name)?)
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
            run_terminal(TerminalManager::with_file(&path_str)?)
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

/// Initialize debug logger writing to ~/.novim/debug.log.
/// TUI can't log to stderr (ratatui uses alternate screen), so we use a file.
fn init_debug_logger() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let log_dir = std::path::PathBuf::from(&home).join(".novim");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("debug.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open debug log file");
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .target(env_logger::Target::Pipe(Box::new(file)))
        .format_timestamp_millis()
        .init();
    log::info!("Debug logging enabled, writing to {}", log_path.display());
}

