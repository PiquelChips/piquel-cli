use crate::{SessionConfig, WindowConfig, config};
use std::io;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum TmuxError {
    Io(io::Error),
    Command(String),
    InTmux,
}

impl std::fmt::Display for TmuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmuxError::Io(e) => write!(f, "IO error: {e}"),
            TmuxError::Command(msg) => write!(f, "{msg}"),
            TmuxError::InTmux => write!(f, "Please do not use this command in tmux"),
        }
    }
}

impl std::error::Error for TmuxError {}

impl From<io::Error> for TmuxError {
    fn from(e: io::Error) -> Self {
        TmuxError::Io(e)
    }
}

/// Lists sessions from tmux, the config, or both — sorted and deduplicated.
pub fn list_sessions(list_config: bool, list_tmux: bool) -> Result<(), TmuxError> {
    let config = config::config();

    let mut sessions: Vec<String> = Vec::new();

    if list_tmux {
        let tmux_sessions = list_tmux_sessions()?;
        sessions.extend(tmux_sessions);
    }

    if list_config {
        for session_name in config.sessions.keys() {
            sessions.push(session_name.clone());
        }
    }

    sessions.sort();
    sessions.dedup();

    for session in &sessions {
        println!("{session}");
    }

    Ok(())
}

/// Returns the names of all running tmux sessions.
pub fn list_tmux_sessions() -> Result<Vec<String>, TmuxError> {
    match exec_tmux_return(&["list-sessions", "-F", "#{session_name}"]) {
        Ok(output) => {
            let trimmed = output.trim_matches('\n');
            if trimmed.is_empty() {
                return Ok(vec![]);
            }
            Ok(trimmed.split('\n').map(str::to_owned).collect())
        }
        Err(TmuxError::Command(ref msg))
            if msg.starts_with("no server running on")
                || msg.starts_with("error connecting to") =>
        {
            Ok(vec![])
        }
        Err(_) => {
            let raw =
                exec_tmux_return(&["list-sessions", "-F", "#{session_name}"]).unwrap_or_default();
            Err(TmuxError::Command(format!(
                "Failed to list sessions with error: {raw}"
            )))
        }
    }
}

/// Attaches to a running tmux session and returns its combined output.
pub fn attach(session: &str) -> Result<String, TmuxError> {
    exec_tmux_return(&["attach", "-t", session])
}

/// Creates a new tmux session (and its windows), then attaches.
pub fn new_session(session_name: &str, session: &SessionConfig) -> Result<(), TmuxError> {
    exec_tmux(&[
        "new-session",
        "-Ad",
        "-c",
        &session.root,
        "-s",
        &session_name,
    ])
    .map_err(|_| {
        TmuxError::Command(format!("Failed to create session with name {session_name}"))
    })?;

    let index = exec_tmux_return(&["list-windows", "-t", &session_name, "-F", "#{window_index}"])
        .map_err(|e| {
        TmuxError::Command(format!("Failed to list tmux windows with error: {e}"))
    })?;

    let index = index.trim_matches('\n').to_owned();

    for (i, window) in session.windows.iter().enumerate() {
        new_window(&session.root, window).map_err(|e| {
            TmuxError::Command(format!("Failed to create window {} with error: {e}", i + 1))
        })?;
    }

    exec_tmux(&["kill-window", "-t", &format!("{session_name}:{index}")])
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;

    exec_tmux(&["select-window", "-t", &format!("{session_name}:{index}")])
        .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;

    attach(&session_name).map_err(|_| {
        TmuxError::Command(format!(
            "Failed to attach to session with error: {session_name}"
        ))
    })?;

    Ok(())
}

/// Creates a new tmux window rooted at `start_dir` and sends its commands.
pub fn new_window(start_dir: &str, window: &WindowConfig) -> Result<(), TmuxError> {
    exec_tmux_return(&["new-window", "-c", start_dir])
        .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;

    for command in &window.commands {
        exec_tmux_return(&["send-keys", command, "Enter"]).map_err(|e| {
            TmuxError::Command(format!(
                "Failed to execute command \"{command}\" with error: {e}"
            ))
        })?;
    }

    Ok(())
}

pub fn in_tmux() -> Result<(), TmuxError> {
    if std::env::var("TMUX").is_ok() {
        Ok(())
    } else {
        Err(TmuxError::InTmux)
    }
}

fn exec_tmux(args: &[&str]) -> Result<(), TmuxError> {
    Command::new("tmux")
        .args(args)
        .stdin(Stdio::inherit())
        .status()
        .map_err(TmuxError::Io)
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(TmuxError::Command(format!(
                    "tmux exited with status {status}"
                )))
            }
        })
}

fn exec_tmux_return(args: &[&str]) -> Result<String, TmuxError> {
    let output = Command::new("tmux")
        .args(args)
        .stdin(Stdio::inherit())
        .output()
        .map_err(TmuxError::Io)?;

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };

    if output.status.success() {
        Ok(combined)
    } else {
        Err(TmuxError::Command(combined))
    }
}
