use std::{
    error::Error,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};

use crate::{
    SessionConfig,
    config::{self, CONFIG_PATH},
    tmux::{self, TmuxError},
};

/// CLI for personal system management
#[derive(Parser, Debug)]
#[command(name = "piquelctl")]
#[command(about = "CLI for system utilities", long_about = None)]
pub struct Cli {
    /// custom path to configuration
    #[arg(
        long = "config",
        value_name = "path",
        default_value = CONFIG_PATH,
        global = true
    )]
    config_path: PathBuf,

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
        Commands::Load { session } => {
            tmux::in_tmux()?;

            let sessions = tmux::list_tmux_sessions()?;

            if sessions.contains(session) {
                match tmux::attach(session) {
                    Ok(_) => return Ok(()),
                    Err(TmuxError::Command(ref msg)) if !msg.starts_with("can't find session:") => {
                        return Err(msg.clone().into());
                    }
                    Err(_) => {}
                }
            }

            let config = config::config();
            let session_config = config.sessions.get(session).ok_or("Invalid session")?;
            tmux::new_session(session, &session_config)?;
        }
        Commands::Session { path } => {
            tmux::in_tmux()?;

            let config = config::config();

            let path = match path {
                Some(path) => match path.to_str() {
                    // TODO: remove when update root to use path
                    Some(path) => path.into(),
                    None => panic!("path is invalid UTF-8 str"),
                },
                None => std::env::var("PWD")?,
            };

            let session = SessionConfig {
                windows: config.default_session.clone(),
                root: path,
            };

            let name_split: Vec<&str> = session.root.split("/").collect();
            let session_name = name_split[name_split.len() - 1];
            tmux::new_session(session_name, &session)?
        }
    };
    Ok(())
}
