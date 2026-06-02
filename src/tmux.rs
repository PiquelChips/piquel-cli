use crate::{SessionConfig, WindowConfig, config};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum TmuxError {
    Io(io::Error),
    Command(String),
    InTmux,
    InvalidSessionName(String),
}

impl std::fmt::Display for TmuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmuxError::Io(e) => write!(f, "IO error: {e}"),
            TmuxError::Command(msg) => write!(f, "{msg}"),
            TmuxError::InTmux => write!(f, "Please do not use this command in tmux"),
            TmuxError::InvalidSessionName(name) => {
                write!(f, "\"{name}\" is not a valid tmux session name")
            }
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
        for project in &config.projects {
            if let Ok(project_name) = project.resolved_name() {
                sessions.push(project_name);
            }
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

/// Opens a tmux session for `root` using `template`, creating it when needed.
pub fn open_session(
    tmux_name: &str,
    root: &Path,
    template: &SessionConfig,
) -> Result<(), TmuxError> {
    let tmux_name = validated_session_name(tmux_name)?;

    let sessions = list_tmux_sessions()?;
    if sessions.contains(&tmux_name) {
        attach(&tmux_name)?;
        return Ok(());
    }

    exec_tmux(&[
        "new-session",
        "-d",
        "-c",
        &root.to_string_lossy(),
        "-s",
        &tmux_name,
    ])
    .map_err(|_| TmuxError::Command(format!("Failed to create session with name {tmux_name}")))?;

    let bootstrap_window =
        exec_tmux_return(&["list-windows", "-t", &tmux_name, "-F", "#{window_id}"]).map_err(
            |e| TmuxError::Command(format!("Failed to list tmux windows with error: {e}")),
        )?;

    let bootstrap_window = bootstrap_window.trim_matches('\n').to_owned();
    let mut first_window = None;

    for (i, window) in template.windows.iter().enumerate() {
        let window_id = new_window(&tmux_name, root, window).map_err(|e| {
            TmuxError::Command(format!("Failed to create window {} with error: {e}", i + 1))
        })?;

        first_window.get_or_insert(window_id);
    }

    exec_tmux(&["kill-window", "-t", &bootstrap_window])
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;

    if let Some(first_window) = first_window {
        exec_tmux(&["select-window", "-t", &first_window])
            .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;
    }

    attach(&tmux_name).map_err(|_| {
        TmuxError::Command(format!(
            "Failed to attach to session with error: {tmux_name}"
        ))
    })?;

    Ok(())
}

/// Creates a new tmux window rooted at `start_dir` and sends its commands.
pub fn new_window(
    session_name: &str,
    start_dir: &Path,
    window: &WindowConfig,
) -> Result<String, TmuxError> {
    let window_id = exec_tmux_return(&[
        "new-window",
        "-P",
        "-F",
        "#{window_id}",
        "-t",
        session_name,
        "-c",
        start_dir.to_str().unwrap(),
    ])
    .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;

    let window_id = window_id.trim_matches('\n').to_owned();

    for command in &window.commands {
        exec_tmux_return(&["send-keys", "-t", &window_id, command, "Enter"]).map_err(|e| {
            TmuxError::Command(format!(
                "Failed to execute command \"{command}\" with error: {e}"
            ))
        })?;
    }

    Ok(window_id)
}

/// Will return an error we are in tmux
pub fn err_in_tmux() -> Result<(), TmuxError> {
    if in_tmux() {
        Err(TmuxError::InTmux)
    } else {
        Ok(())
    }
}

pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

pub fn sanitize_session_name(input: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_underscore = false;

    for ch in input.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ch
        } else {
            '_'
        };

        if next == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
        } else {
            last_was_underscore = false;
        }

        sanitized.push(next);
    }

    sanitized
}

pub fn validated_session_name(input: &str) -> Result<String, TmuxError> {
    let trimmed = input.trim();
    let sanitized = sanitize_session_name(trimmed);
    let has_valid_char = trimmed
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');

    if sanitized.is_empty() || !has_valid_char {
        return Err(TmuxError::InvalidSessionName(input.to_owned()));
    }

    Ok(sanitized)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_tmux_session_names() {
        assert_eq!(sanitize_session_name("project:branch"), "project_branch");
        assert_eq!(sanitize_session_name("feature/foo"), "feature_foo");
        assert_eq!(sanitize_session_name("feature///foo"), "feature_foo");
        assert!(!sanitize_session_name("project:branch").contains(':'));
    }

    #[test]
    fn invalid_tmux_session_names_fail_validation() {
        assert!(validated_session_name("").is_err());
        assert!(validated_session_name("   ").is_err());
        assert!(validated_session_name("///").is_err());
    }
}
