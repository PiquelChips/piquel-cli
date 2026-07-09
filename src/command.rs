use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
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

impl From<CommandInput> for Stdio {
    fn from(value: CommandInput) -> Self {
        match value {
            CommandInput::Closed => Self::null(),
            CommandInput::Inherit => Self::inherit(),
            CommandInput::Bytes(_) => Self::piped(),
        }
    }
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

/// Errors produced while running local commands.
#[derive(Debug, Error)]
pub enum CommandError {
    /// The command process could not be spawned or observed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// A home directory could not be determined.
    #[error("home directory not found")]
    HomeDirNotFound,
    /// The requested program is not available.
    #[error("{0} is not installed or not available in PATH")]
    MissingProgram(String),
}

/// Executes `request` and captures stdout and stderr.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or observed.
pub fn output(request: CommandRequest) -> Result<CommandOutput, CommandError> {
    let stdin = request.stdin_config().clone();
    validate_program(request.program())?;
    let mut child = Command::from(request)
        .stdin(Stdio::from(stdin.clone()))
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

/// Executes `request` while inheriting stdout and stderr.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or observed.
pub fn status(request: CommandRequest) -> Result<ExitStatus, CommandError> {
    let stdin = request.stdin_config().clone();
    validate_program(request.program())?;
    let mut child = Command::from(request)
        .stdin(Stdio::from(stdin.clone()))
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    write_stdin(&mut child, &stdin)?;

    child.wait().map_err(CommandError::Io)
}

/// Executes each request in order while inheriting stdout and stderr.
///
/// # Errors
///
/// Returns an error if any command cannot be spawned or observed.
pub fn statuses(requests: CommandRequests) -> Result<Vec<ExitStatus>, CommandError> {
    requests.into_iter().map(status).collect()
}

/// Returns the current user's home directory.
///
/// # Errors
///
/// Returns an error if a home directory cannot be determined.
pub fn home_dir() -> Result<PathBuf, CommandError> {
    env::home_dir().ok_or(CommandError::HomeDirNotFound)
}

/// Returns the current directory.
///
/// # Errors
///
/// Returns an error if the current directory cannot be determined.
pub fn current_dir() -> Result<PathBuf, CommandError> {
    env::current_dir().map_err(CommandError::Io)
}

/// Expands a leading `~` using the current user's home directory.
///
/// # Errors
///
/// Returns an error if expansion requires a home directory and none is available.
pub fn expand_home(path: &Path) -> Result<PathBuf, CommandError> {
    if let Ok(stripped) = path.strip_prefix("~") {
        return Ok(home_dir()?.join(stripped));
    }

    Ok(path.to_path_buf())
}

/// Validates that `program` is available.
///
/// # Errors
///
/// Returns an error if the program cannot be found.
pub fn validate_program(program: &OsStr) -> Result<(), CommandError> {
    if program_is_path(program) {
        let path = Path::new(program);
        if is_executable_file(path) {
            return Ok(());
        }
    } else if program_path_candidates(program)
        .into_iter()
        .any(|path| is_executable_file(&path))
    {
        return Ok(());
    }

    Err(CommandError::MissingProgram(
        program.to_string_lossy().into_owned(),
    ))
}

fn program_is_path(program: &OsStr) -> bool {
    let path = Path::new(program);
    path.is_absolute() || path.components().count() > 1
}

fn program_path_candidates(program: &OsStr) -> Vec<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(program))
        .collect()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    metadata.permissions().mode() & 0o111 != 0
}

fn write_stdin(child: &mut std::process::Child, stdin: &CommandInput) -> Result<(), CommandError> {
    if let CommandInput::Bytes(input) = stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        match child_stdin.write_all(input) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
            Err(error) => return Err(CommandError::Io(error)),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipes_bytes_to_output_command() {
        let output = output(
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
    fn status_reports_exit_code() {
        let status = status(CommandRequest::new(shell()).args(["-c", "exit 7"]))
            .expect("shell command should run");

        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn validates_path_programs() {
        validate_program(&shell()).expect("shell path should validate");
    }

    #[test]
    fn rejects_missing_programs() {
        let err = validate_program(OsStr::new("piquel-definitely-missing-program"))
            .expect_err("missing program should be rejected");

        assert!(matches!(err, CommandError::MissingProgram(_)));
    }

    fn shell() -> OsString {
        std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"))
    }
}
