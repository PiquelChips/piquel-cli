use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    process::{Command, ExitStatus, Stdio},
};

use thiserror::Error;

/// Standard input configuration for an executed command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandInput {
    /// Inherit stdin from the current process.
    #[default]
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
