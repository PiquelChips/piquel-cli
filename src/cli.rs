use clap::{Parser, Subcommand};
use std::{error::Error, io, path::PathBuf};

use crate::config;

mod sessions;

/// Command-line parser and dispatch.
#[derive(Parser, Debug)]
#[command(name = "piquel")]
#[command(about = "CLI for system utilities", long_about = None)]
pub struct Cli {
    /// custom path to configuration
    #[arg(long = "config", value_name = "path", global = true)]
    config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

/// Top-level CLI commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all available sessions
    List {
        /// Include sessions configured in the JSON config.
        #[arg(short = 'c')]
        list_config: bool,
        /// Include currently running tmux sessions.
        #[arg(short = 't')]
        list_tmux: bool,
    },
    /// Load sessions
    Load {
        /// Name of the configured or running session to load.
        session: String,
    },
    /// Creates a session with default config
    #[command(alias = "s")]
    Session {
        /// Root path for the ad hoc session.
        path: Option<PathBuf>,
    },
}

/// Parses CLI arguments, loads configuration, and dispatches the selected command.
///
/// # Errors
///
/// Returns an error if the default config path cannot be determined, the config
/// cannot be loaded, or the selected command fails.
pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let config_path = match cli.config_path {
        Some(path) => path,
        None => std::env::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?
            .join(".config/piquel/config.json"),
    };

    config::load_config(&config_path)?;

    match &cli.command {
        Commands::List {
            list_config,
            list_tmux,
        } => sessions::list_sessions(*list_config, *list_tmux),
        Commands::Load { session } => sessions::load_session(session),
        Commands::Session { path } => sessions::session(path.clone()),
    }
}
