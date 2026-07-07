use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::{
    Config,
    backend::{Backend, LocalBackend, SshBackend},
    config, tmux,
};

mod projects;
mod sessions;

const CONFIG_ENV_VAR: &str = "PIQUEL_CONFIG";

/// Command-line parser and dispatch.
#[derive(Parser, Debug)]
#[command(name = "piquel")]
#[command(about = "CLI for system utilities", long_about = None)]
pub struct Cli {
    /// custom path to configuration
    #[arg(long = "config", value_name = "path", global = true)]
    config_path: Option<PathBuf>,

    /// configured machine to run commands on over SSH
    #[arg(short = 'm', long = "machine", value_name = "name", global = true)]
    machine: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

/// Top-level CLI commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List running tmux sessions
    List,
    /// Interactively pick a running tmux session or configured project
    Pick {
        /// Project name to open directly.
        project: Option<String>,
        /// Session template override used only when opening a project.
        #[arg(short = 's', long = "session")]
        session: Option<String>,
    },
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

/// Runtime state shared by CLI command handlers.
pub struct State {
    config: Config,
    backend: Box<dyn Backend>,
}

impl State {
    /// Creates CLI state from loaded configuration.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self::with_backend(config, Box::<LocalBackend>::default())
    }

    pub(crate) fn with_backend(config: Config, backend: Box<dyn Backend>) -> Self {
        Self { config, backend }
    }

    pub(crate) fn backend(&self) -> &dyn Backend {
        self.backend.as_ref()
    }

    /// Lists running tmux sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if tmux session listing fails.
    pub fn list(&self) -> Result<()> {
        let mut sessions = tmux::list_sessions(self.backend())?;
        sessions.sort();
        sessions.dedup();

        for session in sessions {
            println!("{session}");
        }

        Ok(())
    }
}

/// Parses CLI arguments, loads configuration, and dispatches the selected command.
///
/// # Errors
///
/// Returns an error if the default config path cannot be determined, the config
/// cannot be loaded, or the selected command fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let local_backend = LocalBackend;

    let config_path = cli
        .config_path
        .or_else(|| std::env::var_os(CONFIG_ENV_VAR).map(PathBuf::from))
        .map_or_else(
            || {
                local_backend
                    .home_dir()
                    .context("home directory not found")
                    .map(|home| home.join(".config/piquel/config.json"))
            },
            Ok,
        )?;

    let mut config = config::read_config(&config_path)?;
    let selected_machine = cli
        .machine
        .as_deref()
        .map(|machine_name| {
            config
                .machine(machine_name)
                .ok_or_else(|| anyhow::anyhow!("Machine \"{machine_name}\" is not configured"))
        })
        .transpose()?;

    if matches!(
        cli.command,
        Commands::Project {
            command: ProjectCommands::List
        }
    ) {
        config.validate_and_normalize(&local_backend)?;

        let mut projects = config
            .projects
            .iter()
            .filter_map(|project| project.resolved_name().ok())
            .collect::<Vec<_>>();

        projects.sort();
        projects.dedup();

        for project in projects {
            println!("{project}");
        }
        return Ok(());
    }

    let backend: Box<dyn Backend> = match selected_machine {
        Some(machine) => Box::new(SshBackend::new(machine)),
        None => Box::new(local_backend),
    };
    config.validate_and_normalize(backend.as_ref())?;
    let state = State::with_backend(config, backend);

    match &cli.command {
        Commands::List => state.list(),
        Commands::Pick { project, session } => state.pick(project.as_deref(), session.as_deref()),
        Commands::Project { command } => match command {
            ProjectCommands::List => Ok(()), // should have been run before state creation
            ProjectCommands::Load {
                project,
                session,
                worktree,
            } => state.load_project(project, session.as_deref(), worktree.as_deref()),
        },
        Commands::Session {
            path,
            session,
            name,
        } => state.session(path.as_deref(), session.as_deref(), name.as_deref()),
    }
}
