//! Data types and helpers for the `piquelcli` command-line tool.

/// Command-line parsing and top-level dispatch.
pub mod cli;
/// JSON config loading and global config access.
pub mod config;
/// Integration helpers for invoking tmux.
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::config::ConfigError;

/// Commands to send to a tmux window after creating it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Commands sent to tmux with `send-keys`.
    pub commands: Vec<String>,
}

/// Configuration for a named tmux session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Root directory used when creating the session and its windows.
    pub root: PathBuf,
    /// Windows to create in this session.
    pub windows: Vec<WindowConfig>,
}

impl SessionConfig {
    fn validate(
        &mut self,
        name: &str,
        validate_session_root: bool,
    ) -> Result<(), config::ConfigError> {
        if name.trim().is_empty() || name.contains('.') {
            return Err(ConfigError::Validation(format!(
                "\"{name}\" is not a valid session name"
            )));
        }

        self.root = expand_home(&self.root);

        if validate_session_root {
            if !self.root.exists() {
                return Err(ConfigError::Validation(format!(
                    "Path {} does not exist",
                    self.root.display()
                )));
            }
            if !self.root.is_dir() {
                return Err(ConfigError::Validation(format!(
                    "Path {} is not a directory",
                    self.root.display()
                )));
            }
        }

        if self.windows.is_empty() {
            return Err(ConfigError::Validation(
                "Session must have at least one window".to_owned(),
            ));
        }

        Ok(())
    }
}

/// Complete JSON configuration for the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Named sessions that can be loaded by the `load` command.
    pub sessions: HashMap<String, SessionConfig>,
    /// Whether configured session roots must exist and be directories.
    pub validate_session_root: bool,
    /// Window definitions used by the ad hoc `session` command.
    pub default_session: Vec<WindowConfig>,
}

/// Replaces '~' with the contents of $HOME
fn expand_home(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~")
        && let Some(home) = std::env::home_dir()
    {
        return home.join(stripped);
    }
    path.to_path_buf()
}
