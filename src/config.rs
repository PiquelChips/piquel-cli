use std::path::{Path, PathBuf};

use crate::Config;
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
}

/// Loads the JSON config from `config_path`.
///
/// # Errors
///
/// Returns an error if the file cannot be read, the JSON cannot be parsed, or
/// validation fails.
pub fn load_config(config_path: &Path) -> Result<Config, ConfigError> {
    let config_file = std::fs::read_to_string(config_path)
        .map_err(|_| ConfigError::FileNotFound(config_path.to_owned()))?;

    let mut parsed: Config = serde_json::from_str(&config_file).map_err(ConfigError::ParseError)?;
    parsed.validate_and_normalize()?;
    Ok(parsed)
}
