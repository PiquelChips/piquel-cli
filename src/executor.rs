use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    process::{Command, ExitStatus, Stdio},
};

use thiserror::Error;

/// Standard input configuration for an executed command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandInput {
    /// Close stdin for the child process.
    #[default]
    Closed,
    /// Inherit stdin from the current process.
    Inherit,
    /// Pipe bytes into command stdin.
    Bytes(Vec<u8>),
}

/// Command execution request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRequest {
    program: OsString,
    args: Vec<OsString>,
    stdin: CommandInput,
}

impl CommandRequest {
    /// Creates a request for `program`.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            stdin: CommandInput::default(),
        }
    }

    /// Adds one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Adds many arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Configures stdin for this command.
    #[must_use]
    pub fn stdin(mut self, stdin: CommandInput) -> Self {
        self.stdin = stdin;
        self
    }

    fn program(&self) -> &OsStr {
        &self.program
    }

    fn args_os(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    fn stdin_config(&self) -> &CommandInput {
        &self.stdin
    }
}

impl From<CommandRequest> for Command {
    fn from(value: CommandRequest) -> Self {
        let mut command = Self::new(value.program());
        command.args(value.args_os());
        command
    }
}

/// Commands to run sequentially.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandRequests {
    requests: Vec<CommandRequest>,
}

impl CommandRequests {
    /// Creates an empty command sequence.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a sequence containing one command.
    #[must_use]
    pub fn one(request: CommandRequest) -> Self {
        Self {
            requests: vec![request],
        }
    }

    /// Appends a command to the sequence.
    pub fn push(&mut self, request: CommandRequest) {
        self.requests.push(request);
    }

    /// Appends many commands to the sequence.
    pub fn extend(&mut self, requests: impl IntoIterator<Item = CommandRequest>) {
        self.requests.extend(requests);
    }
}

impl IntoIterator for CommandRequests {
    type Item = CommandRequest;
    type IntoIter = std::vec::IntoIter<CommandRequest>;

    fn into_iter(self) -> Self::IntoIter {
        self.requests.into_iter()
    }
}

/// Captured command result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Command exit status.
    pub status: ExitStatus,
    /// Captured stdout.
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
}

/// Errors produced by a command executor.
#[derive(Debug, Error)]
pub enum CommandExecutorError {
    /// The command process could not be spawned or observed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Runs commands requested by the CLI.
pub trait CommandExecutor: Send + Sync {
    /// Executes `request` and captures stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned or observed.
    fn output(&self, request: CommandRequest) -> Result<CommandOutput, CommandExecutorError>;

    /// Executes `request` while inheriting stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned or observed.
    fn status(&self, request: CommandRequest) -> Result<ExitStatus, CommandExecutorError>;

    /// Executes each request in order while inheriting stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if any command cannot be spawned or observed.
    fn statuses(&self, requests: CommandRequests) -> Result<Vec<ExitStatus>, CommandExecutorError> {
        requests
            .into_iter()
            .map(|request| self.status(request))
            .collect()
    }
}

/// Local process-backed command executor.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalCommandExecutor;

impl CommandExecutor for LocalCommandExecutor {
    fn output(&self, request: CommandRequest) -> Result<CommandOutput, CommandExecutorError> {
        let stdin = request.stdin_config().clone();
        let mut child = Command::from(request)
            .stdin(stdin_stdio(&stdin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        write_stdin(&mut child, &stdin)?;

        let output = child.wait_with_output()?;

        Ok(CommandOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn status(&self, request: CommandRequest) -> Result<ExitStatus, CommandExecutorError> {
        let stdin = request.stdin_config().clone();
        let mut child = Command::from(request)
            .stdin(stdin_stdio(&stdin))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        write_stdin(&mut child, &stdin)?;

        child.wait().map_err(CommandExecutorError::Io)
    }
}

fn stdin_stdio(stdin: &CommandInput) -> Stdio {
    match stdin {
        CommandInput::Closed => Stdio::null(),
        CommandInput::Inherit => Stdio::inherit(),
        CommandInput::Bytes(_) => Stdio::piped(),
    }
}

fn write_stdin(
    child: &mut std::process::Child,
    stdin: &CommandInput,
) -> Result<(), CommandExecutorError> {
    if let CommandInput::Bytes(input) = stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        match child_stdin.write_all(input) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
            Err(error) => return Err(CommandExecutorError::Io(error)),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn command_requests_close_stdin_by_default() {
        assert_eq!(
            CommandRequest::new("program").stdin_config(),
            &CommandInput::Closed
        );
    }

    #[test]
    fn command_request_builder_preserves_program_args_and_stdin() {
        let request = CommandRequest::new("program")
            .arg("one")
            .args(["two", "three"])
            .stdin(CommandInput::Bytes(b"input".to_vec()));

        assert_eq!(request.program(), OsStr::new("program"));
        assert_eq!(
            request.args_os().collect::<Vec<_>>(),
            vec![OsStr::new("one"), OsStr::new("two"), OsStr::new("three")]
        );
        assert_eq!(
            request.stdin_config(),
            &CommandInput::Bytes(b"input".to_vec())
        );
    }

    #[test]
    fn command_requests_preserve_push_and_extend_order() {
        let mut requests = CommandRequests::one(CommandRequest::new("first"));
        requests.push(CommandRequest::new("second"));
        requests.extend([CommandRequest::new("third"), CommandRequest::new("fourth")]);

        let programs = requests
            .into_iter()
            .map(|request| request.program)
            .collect::<Vec<_>>();

        assert_eq!(programs, vec!["first", "second", "third", "fourth"]);
    }

    #[test]
    fn default_statuses_runs_requests_in_order() {
        let executor = RecordingExecutor::default();
        let statuses = executor
            .statuses({
                let mut requests = CommandRequests::new();
                requests.push(CommandRequest::new("first"));
                requests.push(CommandRequest::new("second"));
                requests
            })
            .expect("statuses should succeed");

        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(ExitStatus::success));
        assert_eq!(
            executor.programs(),
            vec![OsString::from("first"), OsString::from("second")]
        );
    }

    #[test]
    fn local_executor_pipes_bytes_to_output_command() {
        let output = LocalCommandExecutor
            .output(
                CommandRequest::new(shell())
                    .args(["-c", "cat; printf err >&2"])
                    .stdin(CommandInput::Bytes(b"hello".to_vec())),
            )
            .expect("shell command should run");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello");
        assert_eq!(output.stderr, b"err");
    }

    #[test]
    fn local_executor_status_reports_exit_code() {
        let status = LocalCommandExecutor
            .status(CommandRequest::new(shell()).args(["-c", "exit 7"]))
            .expect("shell command should run");

        assert_eq!(status.code(), Some(7));
    }

    #[derive(Default)]
    struct RecordingExecutor {
        programs: Mutex<Vec<OsString>>,
    }

    impl RecordingExecutor {
        fn programs(&self) -> Vec<OsString> {
            self.programs
                .lock()
                .expect("program lock should not be poisoned")
                .clone()
        }
    }

    impl CommandExecutor for RecordingExecutor {
        fn output(&self, _request: CommandRequest) -> Result<CommandOutput, CommandExecutorError> {
            panic!("output should not be called by statuses")
        }

        fn status(&self, request: CommandRequest) -> Result<ExitStatus, CommandExecutorError> {
            self.programs
                .lock()
                .expect("program lock should not be poisoned")
                .push(request.program);
            Ok(status(0))
        }
    }

    fn shell() -> &'static str {
        "/bin/sh"
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(code << 8)
    }
}
