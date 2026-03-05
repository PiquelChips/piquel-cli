use std::{error::Error, path::PathBuf};

use crate::{
    SessionConfig, config,
    tmux::{self, TmuxError},
};

pub fn list_sessions(list_config: bool, list_tmux: bool) -> Result<(), Box<dyn Error>> {
    if !list_tmux && !list_config {
        tmux::list_sessions(true, true)?;
    } else {
        tmux::list_sessions(list_config, list_tmux)?;
    }
    Ok(())
}

pub fn load_session(session: &String) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

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
    Ok(tmux::new_session(session, &session_config)?)
}

pub fn session(path: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

    let config = config::config();

    let path: PathBuf = match path {
        Some(path) => path.to_owned(),
        None => std::env::current_dir()?,
    };

    let session = SessionConfig {
        windows: config.default_session.clone(),
        root: path,
    };

    let root = session.root.to_string_lossy();
    let name_split: Vec<&str> = root.split("/").collect();
    let session_name = name_split[name_split.len() - 1];
    Ok(tmux::new_session(session_name, &session)?)
}
