use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::{config, tmux};

mod projects;
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
    /// List running tmux sessions
    List,
    /// Interactively pick a running tmux session or configured project
    Pick,
    /// Manage configured projects
    Project {
        /// Project subcommand to run.
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Open an arbitrary directory with a session template
    #[command(alias = "s")]
    Session {
        /// Root path for the ad hoc session.
        path: Option<PathBuf>,
        /// Session template to use.
        #[arg(short = 's', long = "session")]
        session: Option<String>,
        /// tmux session name override.
        #[arg(short = 'n', long = "name")]
        name: Option<String>,
    },
}

/// Project management subcommands.
#[derive(Subcommand, Debug)]
pub enum ProjectCommands {
    /// List configured projects
    List,
    /// Load a configured project
    Load {
        /// Project name to load.
        project: String,
        /// Session template override.
        #[arg(short = 's', long = "session")]
        session: Option<String>,
        /// Git worktree branch to open.
        #[arg(short = 't', long = "worktree")]
        worktree: Option<String>,
    },
}

/// Parses CLI arguments, loads configuration, and dispatches the selected command.
///
/// # Errors
///
/// Returns an error if the default config path cannot be determined, the config
/// cannot be loaded, or the selected command fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let config_path = match cli.config_path {
        Some(path) => path,
        None => std::env::home_dir()
            .context("home directory not found")?
            .join(".config/piquel/config.json"),
    };

    config::load_config(&config_path)?;

    match &cli.command {
        Commands::List => {
            tmux::list_sessions(false, true)?;
            Ok(())
        }
        Commands::Pick => sessions::pick(),
        Commands::Project { command } => match command {
            ProjectCommands::List => {
                projects::list_projects();
                Ok(())
            }
            ProjectCommands::Load {
                project,
                session,
                worktree,
            } => projects::load_project(project, session.as_deref(), worktree.as_deref()),
        },
        Commands::Session {
            path,
            session,
            name,
        } => sessions::session(path.clone(), session.as_deref(), name.as_deref()),
    }
}
