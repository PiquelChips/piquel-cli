use std::path::{Path, PathBuf};

use crate::{Config, backend::Backend};
use thiserror::Error;

/// Errors produced while loading or accessing the CLI config.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The configured JSON file could not be found or read.
    #[error("Config file {} does not exist", .0.display())]
    FileNotFound(PathBuf),
    /// The JSON config could not be parsed.
    #[error("Failed to parse config: {0}")]
    ParseError(serde_json::Error),
    /// The parsed config failed semantic validation.
    #[error("{0}")]
    Validation(String),
    /// A backend operation failed while normalizing config.
    #[error("{0}")]
    Backend(#[from] crate::backend::BackendError),
}

/// Loads the JSON config from `config_path`.
///
/// # Errors
///
/// Returns an error if the file cannot be read, the JSON cannot be parsed, or
/// validation fails.
pub fn load_config(config_path: &Path, backend: &dyn Backend) -> Result<Config, ConfigError> {
    let mut parsed = read_config(config_path)?;
    parsed.validate_and_normalize(backend)?;
    Ok(parsed)
}

/// Reads the JSON config from `config_path` without normalizing backend paths.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON cannot be parsed.
pub fn read_config(config_path: &Path) -> Result<Config, ConfigError> {
    let config_file = std::fs::read_to_string(config_path)
        .map_err(|_| ConfigError::FileNotFound(config_path.to_owned()))?;

    serde_json::from_str(&config_file).map_err(ConfigError::ParseError)
}
