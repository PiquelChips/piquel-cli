use std::error::Error;

use clap::{Parser, Subcommand};

/// CLI for personal system management
#[derive(Parser, Debug)]
#[command(name = "piquelctl")]
#[command(about = "CLI for system utilities", long_about = None)]
pub struct Cli {
    /// Path to the Unix socket to connect to
    #[arg(
        short = 'c',
        long = "config",
        value_name = "path",
        default_value = "/home/piquel/.config/piquel/config.yml",
        global = true
    )]
    config_path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all available sessions
    List,
    /// Load sessions
    Load { session: String },
    /// Creates a session with default config
    Session,
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::List => (),
        Commands::Load { session } => (),
        Commands::Session => (),
    };
    Ok(())
}
