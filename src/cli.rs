use clap::{Parser, Subcommand};
use std::{error::Error, path::PathBuf};

use crate::config;

mod sessions;

/// CLI for personal system management
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

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all available sessions
    List {
        #[arg(short = 'c')]
        list_config: bool,
        #[arg(short = 't')]
        list_tmux: bool,
    },
    /// Load sessions
    Load { session: String },
    /// Creates a session with default config
    Session { path: Option<PathBuf> },
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let config_path: &PathBuf = match &cli.config_path {
        Some(path) => path,
        None => &std::env::home_dir()
            .unwrap()
            .join(".config/piquel/config.json"),
    };

    config::load_config(config_path)?;

    match &cli.command {
        Commands::List {
            list_config,
            list_tmux,
        } => sessions::list_sessions(*list_config, *list_tmux),
        Commands::Load { session } => sessions::load_session(session),
        Commands::Session { path } => sessions::session(path.clone()),
    }
}
