pub mod cli;
pub mod config;
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::config::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub root: PathBuf,
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
                    "Path {:?} does not exist",
                    self.root
                )));
            }
            if !self.root.is_dir() {
                return Err(ConfigError::Validation(format!(
                    "Path {:?} is not a directory",
                    self.root
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub sessions: HashMap<String, SessionConfig>,
    pub validate_session_root: bool,
    pub default_session: Vec<WindowConfig>, // a session without a root
}

/// Replaces '~' with the contents of $HOME
fn expand_home(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        if let Some(home) = std::env::home_dir() {
            return home.join(stripped);
        }
    }
    path.to_path_buf()
}
