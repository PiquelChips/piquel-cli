pub mod cli;
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub root: String,
    pub windows: Vec<WindowConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub sessions: HashMap<String, SessionConfig>,
    pub validate_session_root: bool,
    pub default_session: Vec<WindowConfig>, // a session without a root
}
