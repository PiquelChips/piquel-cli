use std::sync::OnceLock;

use crate::Config;

static CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug)]
pub enum ConfigError {
    AlreadyLoaded(String),
    FileNotFound(String),
    ParseError(serde_json::Error),
    Validation(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::AlreadyLoaded(path) => {
                write!(f, "Config has already been loaded from {path}")
            }
            ConfigError::FileNotFound(path) => {
                write!(f, "Config file {path} does not exist")
            }
            ConfigError::ParseError(e) => write!(f, "Failed to parse config: {e}"),
            ConfigError::Validation(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Loads the YAML config from `config_path` into the global `CONFIG`.
/// Returns an error if the config has already been loaded or the file
/// cannot be read.
pub fn load_config(config_path: &str) -> Result<(), ConfigError> {
    if CONFIG.get().is_some() {
        return Err(ConfigError::AlreadyLoaded(config_path.to_owned()));
    }

    let config_file = std::fs::read_to_string(config_path)
        .map_err(|_| ConfigError::FileNotFound(config_path.to_owned()))?;

    let mut parsed: Config = serde_json::from_str(&config_file).map_err(ConfigError::ParseError)?;

    for (name, session) in parsed.sessions.iter_mut() {
        session.validate(name, parsed.validate_session_root)?;
    }

    // `set` fails only if another thread raced us — treat that as already loaded.
    CONFIG
        .set(parsed)
        .map_err(|_| ConfigError::AlreadyLoaded(config_path.to_owned()))
}

/// Returns a reference to the global config.
/// Panics if `load_config` has not been called yet.
pub fn config() -> &'static Config {
    CONFIG.get().expect("Config has not been loaded yet")
}
