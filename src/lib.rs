pub mod cli;
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

use crate::config::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub root: String,
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

        if validate_session_root && !Path::new(&self.root).exists() {
            return Err(ConfigError::Validation(format!(
                "Path {} does not exist",
                self.root
            )));
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
pub fn expand_home(path: &str) -> String {
    if path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen('~', &home, 1);
        }
    }
    path.to_owned()
}
