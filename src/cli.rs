use std::error::Error;

use clap::{Parser, Subcommand};

use crate::{config, tmux};

/// CLI for personal system management
#[derive(Parser, Debug)]
#[command(name = "piquelctl")]
#[command(about = "CLI for system utilities", long_about = None)]
pub struct Cli {
    /// custom path to configuration
    #[arg(
        short = 'c',
        long = "config",
        value_name = "path",
        // TODO: better default
        default_value = "./example_config.json",
        global = true
    )]
    config_path: String,

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
    Session,
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    config::load_config(&cli.config_path)?;

    match &cli.command {
        Commands::List {
            list_config,
            list_tmux,
        } => {
            if !list_tmux && !list_config {
                tmux::list_sessions(true, true)?;
            } else {
                tmux::list_sessions(*list_config, *list_tmux)?;
            }
        }
        Commands::Load { session } => println!("Loading {session}"),
        Commands::Session => println!("Creating new session"),
    };
    Ok(())
}
