use crate::{
    SessionConfig, WindowConfig,
    executor::{
        CommandExecutor, CommandExecutorError, CommandInput, CommandOutput, CommandRequest,
        CommandRequests,
    },
};
use std::io;
use std::path::Path;
use std::process::ExitStatus;
use thiserror::Error;

/// Errors produced while invoking tmux.
#[derive(Debug, Error)]
pub enum TmuxError {
    /// The tmux process could not be spawned or observed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// tmux exited unsuccessfully or returned an unexpected response.
    #[error("{0}")]
    Command(String),
    /// The command cannot run from inside an existing tmux session.
    #[error("Please do not use this command in tmux")]
    InTmux,
    /// The requested tmux session name cannot be sanitized into a valid name.
    #[error("\"{0}\" is not a valid tmux session name")]
    InvalidSessionName(String),
}

impl From<CommandExecutorError> for TmuxError {
    fn from(value: CommandExecutorError) -> Self {
        match value {
            CommandExecutorError::Io(err) => TmuxError::Io(err),
        }
    }
}

/// Returns the command request for listing tmux sessions.
#[must_use]
pub fn list_sessions_request() -> CommandRequest {
    tmux_request(["list-sessions", "-F", "#{session_name}"])
}

/// Lists running tmux sessions through `executor`.
///
/// # Errors
///
/// Returns an error if tmux cannot be invoked or returns an unexpected failure.
pub fn list_sessions(executor: &dyn CommandExecutor) -> Result<Vec<String>, TmuxError> {
    let output = executor.output(list_sessions_request())?;
    parse_list_sessions_output(&output)
}

/// Parses tmux session names from `list-sessions` output.
///
/// # Errors
///
/// Returns an error if tmux failed for a reason other than a missing server.
pub fn parse_list_sessions_output(output: &CommandOutput) -> Result<Vec<String>, TmuxError> {
    let combined = combined_output(output);

    if !output.status.success() {
        if combined.starts_with("no server running on")
            || combined.starts_with("error connecting to")
        {
            return Ok(vec![]);
        }

        return Err(TmuxError::Command(format!(
            "Failed to list sessions with error: {combined}"
        )));
    }

    let trimmed = combined.trim_matches('\n');
    if trimmed.is_empty() {
        return Ok(vec![]);
    }

    Ok(trimmed.split('\n').map(str::to_owned).collect())
}

/// Returns the command request for attaching to a tmux session.
#[must_use]
pub fn attach_request(session: &str) -> CommandRequest {
    tmux_request(["attach", "-t", session])
}

/// Attaches to an existing tmux session through `executor`.
///
/// # Errors
///
/// Returns an error if tmux cannot attach to the requested session.
pub fn attach(executor: &dyn CommandExecutor, name: &str) -> Result<String, TmuxError> {
    let output = executor.output(attach_request(name))?;
    successful_output(&output)
}

/// Returns the command request for creating a detached tmux session.
#[must_use]
pub fn new_session_request(tmux_name: &str, root: &Path) -> CommandRequest {
    tmux_request([
        "new-session",
        "-d",
        "-c",
        &root.to_string_lossy(),
        "-s",
        tmux_name,
    ])
}

/// Returns the command request for listing tmux windows.
#[must_use]
pub fn list_windows_request(tmux_name: &str) -> CommandRequest {
    tmux_request(["list-windows", "-t", tmux_name, "-F", "#{window_id}"])
}

/// Returns the command request for creating a tmux window.
#[must_use]
pub fn new_window_request(
    session_name: &str,
    start_dir: &Path,
    window: &WindowConfig,
) -> CommandRequest {
    let mut request = tmux_request(["new-window", "-P", "-F", "#{window_id}"]);

    if let Some(name) = &window.name {
        request = request.args(["-n", name]);
    }

    request.args([
        "-t",
        session_name,
        "-c",
        start_dir.to_string_lossy().as_ref(),
    ])
}

/// Returns the command request for sending keys to a tmux window.
#[must_use]
pub fn send_keys_request(window_id: &str, command: &str) -> CommandRequest {
    tmux_request(["send-keys", "-t", window_id, command, "Enter"])
}

/// Returns command requests for all configured commands in a tmux window.
#[must_use]
pub fn send_keys_requests(window_id: &str, window: &WindowConfig) -> CommandRequests {
    let mut requests = CommandRequests::new();
    requests.extend(
        window
            .commands
            .iter()
            .map(|command| send_keys_request(window_id, command)),
    );
    requests
}

/// Returns the command request for killing a tmux window.
#[must_use]
pub fn kill_window_request(window_id: &str) -> CommandRequest {
    tmux_request(["kill-window", "-t", window_id])
}

/// Returns the command request for selecting a tmux window.
#[must_use]
pub fn select_window_request(window_id: &str) -> CommandRequest {
    tmux_request(["select-window", "-t", window_id])
}

/// Opens a tmux session from `template`, creating it first when necessary.
///
/// # Errors
///
/// Returns an error if the session name is invalid or any tmux command fails.
pub fn open_session(
    executor: &dyn CommandExecutor,
    tmux_name: &str,
    root: &Path,
    template: &SessionConfig,
) -> Result<(), TmuxError> {
    let tmux_name = validated_session_name(tmux_name)?;

    let sessions = list_sessions(executor)?;
    if sessions.contains(&tmux_name) {
        attach(executor, &tmux_name)?;
        return Ok(());
    }

    let status = executor
        .status(new_session_request(&tmux_name, root))
        .map_err(|_| {
            TmuxError::Command(format!("Failed to create session with name {tmux_name}"))
        })?;
    successful_status(status).map_err(|_| {
        TmuxError::Command(format!("Failed to create session with name {tmux_name}"))
    })?;

    let output = executor
        .output(list_windows_request(&tmux_name))
        .map_err(|e| TmuxError::Command(format!("Failed to list tmux windows with error: {e}")))?;
    let bootstrap_window = successful_output(&output)
        .map_err(|e| TmuxError::Command(format!("Failed to list tmux windows with error: {e}")))?;
    let bootstrap_window = bootstrap_window.trim_matches('\n').to_owned();
    let mut first_window = None;

    for (i, window) in template.windows.iter().enumerate() {
        let window_id = create_window(executor, &tmux_name, root, window).map_err(|e| {
            TmuxError::Command(format!("Failed to create window {} with error: {e}", i + 1))
        })?;

        first_window.get_or_insert(window_id);
    }

    let status = executor
        .status(kill_window_request(&bootstrap_window))
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;
    successful_status(status)
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;

    if let Some(first_window) = first_window {
        let status = executor
            .status(select_window_request(&first_window))
            .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;
        successful_status(status)
            .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;
    }

    attach(executor, &tmux_name).map_err(|_| {
        TmuxError::Command(format!(
            "Failed to attach to session with error: {tmux_name}"
        ))
    })?;

    Ok(())
}

/// Converts successful command output into combined stdout and stderr text.
///
/// # Errors
///
/// Returns an error if the command status was unsuccessful.
pub fn successful_output(output: &CommandOutput) -> Result<String, TmuxError> {
    let combined = combined_output(output);
    if output.status.success() {
        Ok(combined)
    } else {
        Err(TmuxError::Command(combined))
    }
}

/// Converts a command exit status into a tmux result.
///
/// # Errors
///
/// Returns an error if the command status was unsuccessful.
pub fn successful_status(status: ExitStatus) -> Result<(), TmuxError> {
    if status.success() {
        Ok(())
    } else {
        Err(TmuxError::Command(format!(
            "tmux exited with status {status}"
        )))
    }
}

/// Converts multiple command exit statuses into a tmux result.
///
/// # Errors
///
/// Returns an error if any command status was unsuccessful.
pub fn successful_statuses(statuses: Vec<ExitStatus>) -> Result<(), TmuxError> {
    for status in statuses {
        successful_status(status)?;
    }

    Ok(())
}

/// Returns an error when the current process is already running inside tmux.
///
/// # Errors
///
/// Returns [`TmuxError::InTmux`] if the `TMUX` environment variable is set.
pub fn err_in_tmux() -> Result<(), TmuxError> {
    if in_tmux() {
        Err(TmuxError::InTmux)
    } else {
        Ok(())
    }
}

/// Returns whether the current process is running inside tmux.
#[must_use]
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Sanitizes arbitrary text into a tmux-compatible session name.
#[must_use]
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

/// Validates and sanitizes a tmux session name.
///
/// # Errors
///
/// Returns an error if `input` contains no valid tmux session-name characters.
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

fn tmux_request<'a>(args: impl IntoIterator<Item = &'a str>) -> CommandRequest {
    CommandRequest::new("tmux")
        .args(args)
        .stdin(CommandInput::Inherit)
}

fn create_window(
    executor: &dyn CommandExecutor,
    session_name: &str,
    start_dir: &Path,
    window: &WindowConfig,
) -> Result<String, TmuxError> {
    let output = executor
        .output(new_window_request(session_name, start_dir, window))
        .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;
    let window_id = successful_output(&output)
        .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;
    let window_id = window_id.trim_matches('\n').to_owned();

    for command in &window.commands {
        let output = executor
            .output(send_keys_request(&window_id, command))
            .map_err(|e| {
                TmuxError::Command(format!(
                    "Failed to execute command \"{command}\" with error: {e}"
                ))
            })?;
        successful_output(&output).map_err(|e| {
            TmuxError::Command(format!(
                "Failed to execute command \"{command}\" with error: {e}"
            ))
        })?;
    }

    Ok(window_id)
}

fn combined_output(output: &CommandOutput) -> String {
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    combined
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
