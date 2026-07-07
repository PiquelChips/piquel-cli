use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

use thiserror::Error;

mod local;
pub use local::LocalBackend;

mod ssh;
pub use ssh::SshBackend;

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

/// Errors produced by a backend.
#[derive(Debug, Error)]
pub enum BackendError {
    /// The command process could not be spawned or observed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// The backend could not determine a home directory.
    #[error("home directory not found")]
    HomeDirNotFound,
    /// The requested program is not available to this backend.
    #[error("{0} is not installed or not available in PATH")]
    MissingProgram(String),
}

/// Performs machine interactions requested by the CLI.
pub trait Backend: Send + Sync {
    /// Executes `request` and captures stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned or observed.
    fn output(&self, request: CommandRequest) -> Result<CommandOutput, BackendError>;

    /// Executes `request` while inheriting stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned or observed.
    fn status(&self, request: CommandRequest) -> Result<ExitStatus, BackendError>;

    /// Executes each request in order while inheriting stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an error if any command cannot be spawned or observed.
    fn statuses(&self, requests: CommandRequests) -> Result<Vec<ExitStatus>, BackendError> {
        requests
            .into_iter()
            .map(|request| self.status(request))
            .collect()
    }

    /// Returns the backend user's home directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot determine a home directory.
    fn home_dir(&self) -> Result<PathBuf, BackendError>;

    /// Returns the backend's current directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the current directory cannot be determined.
    fn current_dir(&self) -> Result<PathBuf, BackendError>;

    /// Expands a leading `~` using the backend user's home directory.
    ///
    /// # Errors
    ///
    /// Returns an error if expansion requires a home directory and none is
    /// available.
    fn expand_home(&self, path: &Path) -> Result<PathBuf, BackendError> {
        if let Ok(stripped) = path.strip_prefix("~") {
            return Ok(self.home_dir()?.join(stripped));
        }

        Ok(path.to_path_buf())
    }

    /// Returns whether `path` exists on the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot query the path.
    fn path_exists(&self, path: &Path) -> Result<bool, BackendError>;

    /// Returns whether `path` is a directory on the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot query the path.
    fn path_is_dir(&self, path: &Path) -> Result<bool, BackendError>;

    /// Creates `path` and any missing parent directories on the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    fn create_dir_all(&self, path: &Path) -> Result<(), BackendError>;

    /// Canonicalizes `path` on the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be canonicalized.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, BackendError>;

    /// Validates that `program` is available to this backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be found.
    fn validate_program(&self, program: &OsStr) -> Result<(), BackendError>;
}

#[cfg(test)]
fn shell() -> OsString {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .flat_map(|path| [path.join("sh"), path.join("bash")])
        .find(|path| path.exists())
        .expect("test shell should be available in PATH")
        .into_os_string()
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
        let backend = RecordingBackend::default();
        let statuses = backend
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
            backend.programs(),
            vec![OsString::from("first"), OsString::from("second")]
        );
    }

    #[derive(Default)]
    struct RecordingBackend {
        programs: Mutex<Vec<OsString>>,
    }

    impl RecordingBackend {
        fn programs(&self) -> Vec<OsString> {
            self.programs
                .lock()
                .expect("program lock should not be poisoned")
                .clone()
        }
    }

    impl Backend for RecordingBackend {
        fn output(&self, _request: CommandRequest) -> Result<CommandOutput, BackendError> {
            panic!("output should not be called by statuses")
        }

        fn status(&self, request: CommandRequest) -> Result<ExitStatus, BackendError> {
            self.programs
                .lock()
                .expect("program lock should not be poisoned")
                .push(request.program);
            Ok(status(0))
        }

        fn home_dir(&self) -> Result<PathBuf, BackendError> {
            Ok(PathBuf::from("/home/test"))
        }

        fn current_dir(&self) -> Result<PathBuf, BackendError> {
            Ok(PathBuf::from("/repo"))
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

        fn canonicalize(&self, path: &Path) -> Result<PathBuf, BackendError> {
            Ok(path.to_path_buf())
        }

        fn validate_program(&self, _program: &OsStr) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(code << 8)
    }
}
