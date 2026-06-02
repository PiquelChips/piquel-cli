use std::{
    error::Error,
    path::{Path, PathBuf},
};

use crate::{config, tmux};

pub fn session(
    path: Option<PathBuf>,
    session_override: Option<&str>,
    name_override: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

    let config = config::config();
    let template_name = session_override.unwrap_or(&config.default_session);
    let template = config
        .session_template(template_name)
        .ok_or_else(|| format!("Session template \"{template_name}\" is not configured"))?;

    let root = match path {
        Some(path) => expand_home(&path),
        None => std::env::current_dir()?,
    };

    if !root.exists() {
        return Err(format!("Session path {root:?} does not exist").into());
    }

    if !root.is_dir() {
        return Err(format!("Session path {root:?} is not a directory").into());
    }

    let tmux_name = match name_override {
        Some(name) => name.to_owned(),
        None => root
            .file_name()
            .ok_or_else(|| format!("Could not derive session name from path {root:?}"))?
            .to_string_lossy()
            .into_owned(),
    };

    tmux::open_session(&tmux_name, &root, template)?;
    Ok(())
}

fn expand_home(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~")
        && let Some(home) = std::env::home_dir()
    {
        return home.join(stripped);
    }
    path.to_path_buf()
}
