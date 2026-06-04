use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::Config;
use thiserror::Error;

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Errors produced while loading or accessing the CLI config.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The process-global config was already initialized.
    #[error("Config has already been loaded")]
    AlreadyLoaded,
    /// The configured JSON file could not be found or read.
    #[error("Config file {} does not exist", .0.display())]
    FileNotFound(PathBuf),
    /// The JSON config could not be parsed.
    #[error("Failed to parse config: {0}")]
    ParseError(serde_json::Error),
    /// The parsed config failed semantic validation.
    #[error("{0}")]
    Validation(String),
}

/// Loads the JSON config from `config_path` into the global config store.
///
/// # Errors
///
/// Returns an error if the config has already been loaded, the file cannot be
/// read, the JSON cannot be parsed, or validation fails.
pub fn load_config(config_path: &Path) -> Result<(), ConfigError> {
    if CONFIG.get().is_some() {
        return Err(ConfigError::AlreadyLoaded);
    }

    let config_file = std::fs::read_to_string(config_path)
        .map_err(|_| ConfigError::FileNotFound(config_path.to_owned()))?;

    let mut parsed: Config = serde_json::from_str(&config_file).map_err(ConfigError::ParseError)?;
    parsed.validate_and_normalize()?;

    // `set` fails only if another thread raced us — treat that as already loaded.
    CONFIG.set(parsed).map_err(|_| ConfigError::AlreadyLoaded)
}

/// Returns a reference to the global config.
///
/// # Panics
///
/// Panics if [`load_config`] has not been called successfully.
pub fn config() -> &'static Config {
    CONFIG.get().expect("Config has not been loaded yet")
}
