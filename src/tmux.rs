use crate::{
    SessionConfig, WindowConfig,
    backend::{
        Backend, BackendError, CommandInput, CommandOutput, CommandRequest, CommandRequests,
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
    /// A backend operation failed.
    #[error("{0}")]
    Backend(BackendError),
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

impl From<BackendError> for TmuxError {
    fn from(value: BackendError) -> Self {
        match value {
            BackendError::Io(err) => TmuxError::Io(err),
            error => TmuxError::Backend(error),
        }
    }
}

/// Returns the command request for listing tmux sessions.
#[must_use]
pub fn list_sessions_request() -> CommandRequest {
    tmux_request(["list-sessions", "-F", "#{session_name}"])
}

/// Lists running tmux sessions through `backend`.
///
/// # Errors
///
/// Returns an error if tmux cannot be invoked or returns an unexpected failure.
pub fn list_sessions(backend: &dyn Backend) -> Result<Vec<String>, TmuxError> {
    let output = backend.output(list_sessions_request())?;
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

/// Attaches to an existing tmux session through `backend`.
///
/// # Errors
///
/// Returns an error if tmux cannot attach to the requested session.
pub fn attach(backend: &dyn Backend, name: &str) -> Result<String, TmuxError> {
    let output = backend.output(attach_request(name))?;
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
    backend: &dyn Backend,
    tmux_name: &str,
    root: &Path,
    template: &SessionConfig,
) -> Result<(), TmuxError> {
    let tmux_name = validated_session_name(tmux_name)?;

    let sessions = list_sessions(backend)?;
    if sessions.contains(&tmux_name) {
        attach(backend, &tmux_name)?;
        return Ok(());
    }

    let status = backend
        .status(new_session_request(&tmux_name, root))
        .map_err(|_| {
            TmuxError::Command(format!("Failed to create session with name {tmux_name}"))
        })?;
    successful_status(status).map_err(|_| {
        TmuxError::Command(format!("Failed to create session with name {tmux_name}"))
    })?;

    let output = backend
        .output(list_windows_request(&tmux_name))
        .map_err(|e| TmuxError::Command(format!("Failed to list tmux windows with error: {e}")))?;
    let bootstrap_window = successful_output(&output)
        .map_err(|e| TmuxError::Command(format!("Failed to list tmux windows with error: {e}")))?;
    let bootstrap_window = bootstrap_window.trim_matches('\n').to_owned();
    let mut first_window = None;

    for (i, window) in template.windows.iter().enumerate() {
        let window_id = create_window(backend, &tmux_name, root, window).map_err(|e| {
            TmuxError::Command(format!("Failed to create window {} with error: {e}", i + 1))
        })?;

        first_window.get_or_insert(window_id);
    }

    let status = backend
        .status(kill_window_request(&bootstrap_window))
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;
    successful_status(status)
        .map_err(|_| TmuxError::Command("Failed to kill first window".to_owned()))?;

    if let Some(first_window) = first_window {
        let status = backend
            .status(select_window_request(&first_window))
            .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;
        successful_status(status)
            .map_err(|_| TmuxError::Command("Failed to select first window".to_owned()))?;
    }

    attach(backend, &tmux_name).map_err(|_| {
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
    backend: &dyn Backend,
    session_name: &str,
    start_dir: &Path,
    window: &WindowConfig,
) -> Result<String, TmuxError> {
    let output = backend
        .output(new_window_request(session_name, start_dir, window))
        .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;
    let window_id = successful_output(&output)
        .map_err(|e| TmuxError::Command(format!("Failed to create window with error: {e}")))?;
    let window_id = window_id.trim_matches('\n').to_owned();

    for command in &window.commands {
        let output = backend
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
    use std::{collections::VecDeque, sync::Mutex};

    #[test]
    fn sanitizes_tmux_session_names() {
        assert_eq!(sanitize_session_name("project:branch"), "project_branch");
        assert_eq!(sanitize_session_name("feature/foo"), "feature_foo");
        assert_eq!(sanitize_session_name("feature///foo"), "feature_foo");
        assert_eq!(sanitize_session_name("  feature/foo  "), "feature_foo");
        assert_eq!(sanitize_session_name("release--2026"), "release--2026");
        assert_eq!(sanitize_session_name("release__2026"), "release_2026");
        assert!(!sanitize_session_name("project:branch").contains(':'));
    }

    #[test]
    fn invalid_tmux_session_names_fail_validation() {
        assert!(validated_session_name("").is_err());
        assert!(validated_session_name("   ").is_err());
        assert!(validated_session_name("///").is_err());
    }

    #[test]
    fn valid_tmux_session_names_are_sanitized_during_validation() {
        assert_eq!(
            validated_session_name(" feature/foo:bar ").expect("name should validate"),
            "feature_foo_bar"
        );
    }

    #[test]
    fn request_builders_construct_expected_tmux_commands() {
        let window = WindowConfig {
            name: Some("editor".to_owned()),
            commands: vec!["vim .".to_owned(), "cargo test".to_owned()],
        };

        assert_eq!(
            list_sessions_request(),
            tmux_request(["list-sessions", "-F", "#{session_name}"])
        );
        assert_eq!(
            attach_request("alpha"),
            tmux_request(["attach", "-t", "alpha"])
        );
        assert_eq!(
            new_session_request("alpha", Path::new("/repo")),
            tmux_request(["new-session", "-d", "-c", "/repo", "-s", "alpha"])
        );
        assert_eq!(
            list_windows_request("alpha"),
            tmux_request(["list-windows", "-t", "alpha", "-F", "#{window_id}"])
        );
        assert_eq!(
            new_window_request("alpha", Path::new("/repo"), &window),
            tmux_request([
                "new-window",
                "-P",
                "-F",
                "#{window_id}",
                "-n",
                "editor",
                "-t",
                "alpha",
                "-c",
                "/repo"
            ])
        );
        assert_eq!(
            send_keys_request("@1", "cargo test"),
            tmux_request(["send-keys", "-t", "@1", "cargo test", "Enter"])
        );
        assert_eq!(
            kill_window_request("@0"),
            tmux_request(["kill-window", "-t", "@0"])
        );
        assert_eq!(
            select_window_request("@1"),
            tmux_request(["select-window", "-t", "@1"])
        );

        let requests = send_keys_requests("@1", &window)
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(
            requests,
            vec![
                send_keys_request("@1", "vim ."),
                send_keys_request("@1", "cargo test")
            ]
        );
    }

    #[test]
    fn list_sessions_treats_missing_tmux_server_as_empty() {
        for message in [
            "no server running on /tmp/tmux-1000/default\n",
            "error connecting to /tmp/tmux-1000/default (No such file or directory)\n",
        ] {
            let output = command_output(1, b"", message.as_bytes());

            assert!(
                parse_list_sessions_output(&output)
                    .expect("missing server should parse")
                    .is_empty()
            );
        }
    }

    #[test]
    fn list_sessions_combines_stdout_and_stderr_on_unexpected_failure() {
        let output = command_output(1, b"stdout\n", b"stderr\n");
        let err = parse_list_sessions_output(&output).expect_err("failure should be returned");

        assert!(err.to_string().contains("stdout"));
        assert!(err.to_string().contains("stderr"));
    }

    #[test]
    fn list_sessions_trims_only_outer_newlines() {
        let output = command_output(0, b"alpha\nbeta\n", b"");

        let sessions = parse_list_sessions_output(&output).expect("sessions should parse");

        assert_eq!(sessions, vec!["alpha", "beta"]);
    }

    #[test]
    fn successful_output_combines_streams_and_rejects_failures() {
        let success = command_output(0, b"stdout", b"stderr");
        assert_eq!(
            successful_output(&success).expect("success should parse"),
            "stdoutstderr"
        );

        let failure = command_output(1, b"stdout", b"stderr");
        let err = successful_output(&failure).expect_err("failure should be returned");
        assert_eq!(err.to_string(), "stdoutstderr");
    }

    #[test]
    fn successful_statuses_rejects_first_failed_status() {
        let err = successful_statuses(vec![status(0), status(1), status(0)])
            .expect_err("failed status should be returned");

        assert!(matches!(err, TmuxError::Command(_)));
    }

    #[test]
    fn open_session_attaches_when_session_already_exists() {
        let backend = RecordingBackend::new(vec![
            command_output(0, b"alpha\nbeta\n", b""),
            command_output(0, b"", b""),
        ]);
        let template = SessionConfig {
            windows: vec![WindowConfig {
                name: None,
                commands: vec!["should not run".to_owned()],
            }],
        };

        open_session(&backend, "alpha", Path::new("/repo"), &template)
            .expect("existing session should attach");

        assert_eq!(
            backend.output_requests(),
            vec![list_sessions_request(), attach_request("alpha")]
        );
        assert!(backend.status_requests().is_empty());
    }

    #[test]
    fn open_session_creates_windows_selects_first_and_attaches() {
        let backend = RecordingBackend::new(vec![
            command_output(0, b"", b""),
            command_output(0, b"@0\n", b""),
            command_output(0, b"@1\n", b""),
            command_output(0, b"", b""),
            command_output(0, b"@2\n", b""),
            command_output(0, b"", b""),
            command_output(0, b"", b""),
        ]);
        let template = SessionConfig {
            windows: vec![
                WindowConfig {
                    name: Some("editor".to_owned()),
                    commands: vec!["vim .".to_owned()],
                },
                WindowConfig {
                    name: None,
                    commands: vec!["cargo test".to_owned()],
                },
            ],
        };

        open_session(&backend, "feature/foo", Path::new("/repo"), &template)
            .expect("new session should be created");

        assert_eq!(
            backend.output_requests(),
            vec![
                list_sessions_request(),
                list_windows_request("feature_foo"),
                new_window_request("feature_foo", Path::new("/repo"), &template.windows[0]),
                send_keys_request("@1", "vim ."),
                new_window_request("feature_foo", Path::new("/repo"), &template.windows[1]),
                send_keys_request("@2", "cargo test"),
                attach_request("feature_foo"),
            ]
        );
        assert_eq!(
            backend.status_requests(),
            vec![
                new_session_request("feature_foo", Path::new("/repo")),
                kill_window_request("@0"),
                select_window_request("@1"),
            ]
        );
    }

    struct RecordingBackend {
        output_responses: Mutex<VecDeque<CommandOutput>>,
        output_requests: Mutex<Vec<CommandRequest>>,
        status_requests: Mutex<Vec<CommandRequest>>,
    }

    impl RecordingBackend {
        fn new(output_responses: Vec<CommandOutput>) -> Self {
            Self {
                output_responses: Mutex::new(output_responses.into()),
                output_requests: Mutex::new(Vec::new()),
                status_requests: Mutex::new(Vec::new()),
            }
        }

        fn output_requests(&self) -> Vec<CommandRequest> {
            self.output_requests
                .lock()
                .expect("requests lock should not be poisoned")
                .clone()
        }

        fn status_requests(&self) -> Vec<CommandRequest> {
            self.status_requests
                .lock()
                .expect("requests lock should not be poisoned")
                .clone()
        }
    }

    impl Backend for RecordingBackend {
        fn output(&self, request: CommandRequest) -> Result<CommandOutput, BackendError> {
            self.output_requests
                .lock()
                .expect("requests lock should not be poisoned")
                .push(request);
            Ok(self
                .output_responses
                .lock()
                .expect("responses lock should not be poisoned")
                .pop_front()
                .expect("test output response should exist"))
        }

        fn status(&self, request: CommandRequest) -> Result<ExitStatus, BackendError> {
            self.status_requests
                .lock()
                .expect("requests lock should not be poisoned")
                .push(request);
            Ok(status(0))
        }

        fn home_dir(&self) -> Result<std::path::PathBuf, BackendError> {
            Ok(std::path::PathBuf::from("/home/test"))
        }

        fn current_dir(&self) -> Result<std::path::PathBuf, BackendError> {
            Ok(std::path::PathBuf::from("/repo"))
        }

        fn path_exists(&self, _path: &Path) -> Result<bool, BackendError> {
            Ok(true)
        }

        fn path_is_dir(&self, _path: &Path) -> Result<bool, BackendError> {
            Ok(true)
        }

        fn create_dir_all(&self, _path: &Path) -> Result<(), BackendError> {
            Ok(())
        }

        fn canonicalize(&self, path: &Path) -> Result<std::path::PathBuf, BackendError> {
            Ok(path.to_path_buf())
        }

        fn validate_program(&self, _program: &std::ffi::OsStr) -> Result<(), BackendError> {
            Ok(())
        }
    }

    fn command_output(code: i32, stdout: &[u8], stderr: &[u8]) -> CommandOutput {
        CommandOutput {
            status: status(code),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(code << 8)
    }
}
