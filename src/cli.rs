use clap::{Parser, Subcommand};
use std::{error::Error, path::PathBuf};

use crate::{config, tmux};

mod projects;
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
    /// List running tmux sessions
    List,
    /// Manage configured projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Open an arbitrary directory with a session template
    #[command(alias = "s")]
    Session {
        path: Option<PathBuf>,
        #[arg(short = 's', long = "session")]
        session: Option<String>,
        #[arg(short = 'n', long = "name")]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommands {
    /// List configured projects
    List,
    /// Load a configured project
    Load {
        project: String,
        #[arg(short = 's', long = "session")]
        session: Option<String>,
        #[arg(short = 't', long = "worktree")]
        worktree: Option<String>,
    },
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
        Commands::List => tmux::list_sessions(false, true).map_err(Into::into),
        Commands::Project { command } => match command {
            ProjectCommands::List => projects::list_projects(),
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
