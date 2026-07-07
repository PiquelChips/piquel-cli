use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitStatus,
};

use crate::MachineConfig;

use super::{Backend, BackendError, CommandInput, CommandOutput, CommandRequest, LocalBackend};

/// SSH process-backed backend for a configured remote machine.
#[derive(Debug, Clone)]
pub struct SshBackend {
    local: LocalBackend,
    target: String,
}

impl SshBackend {
    /// Creates an SSH backend for `machine`.
    #[must_use]
    pub fn new(machine: &MachineConfig) -> Self {
        Self {
            target: format!("{}@{}", machine.username(), machine.address()),
            local: LocalBackend,
        }
    }

    fn ssh_request(
        &self,
        request: &CommandRequest,
        stdin: CommandInput,
        allocate_tty: bool,
    ) -> CommandRequest {
        let mut parts = vec![shell_quote_os(request.program())];
        parts.extend(request.args_os().map(shell_quote_os));
        let remote_command = parts.join(" ");
        let tty_flag = if allocate_tty { "-t" } else { "-T" };

        CommandRequest::new("ssh")
            .args([tty_flag, "--", &self.target, &remote_command])
            .stdin(stdin)
    }

    fn shell_status(&self, request: &CommandRequest) -> Result<ExitStatus, BackendError> {
        self.local
            .status(self.ssh_request(request, CommandInput::Closed, false))
    }

    fn shell_output(&self, request: &CommandRequest) -> Result<CommandOutput, BackendError> {
        self.local
            .output(self.ssh_request(request, CommandInput::Closed, false))
    }

    fn bool_status(&self, request: &CommandRequest) -> Result<bool, BackendError> {
        let status = self.shell_status(request)?;
        Ok(status.success())
    }
}

impl Backend for SshBackend {
    fn output(&self, request: CommandRequest) -> Result<CommandOutput, BackendError> {
        self.validate_program(request.program())?;
        self.local
            .output(self.ssh_request(&request, request.stdin_config().clone(), false))
    }

    fn status(&self, request: CommandRequest) -> Result<ExitStatus, BackendError> {
        self.validate_program(request.program())?;
        let allocate_tty = matches!(request.stdin_config(), CommandInput::Inherit);
        self.local
            .status(self.ssh_request(&request, request.stdin_config().clone(), allocate_tty))
    }

    fn home_dir(&self) -> Result<PathBuf, BackendError> {
        let output = self.shell_output(&shell_request("printf '%s\\n' \"$HOME\""))?;
        successful_output_path(&output)
    }

    fn current_dir(&self) -> Result<PathBuf, BackendError> {
        let output = self.shell_output(&shell_request("pwd -P"))?;
        successful_output_path(&output)
    }

    fn path_exists(&self, path: &Path) -> Result<bool, BackendError> {
        self.bool_status(&shell_request(&format!(
            "test -e {}",
            shell_quote_path(path)
        )))
    }

    fn path_is_dir(&self, path: &Path) -> Result<bool, BackendError> {
        self.bool_status(&shell_request(&format!(
            "test -d {}",
            shell_quote_path(path)
        )))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), BackendError> {
        let status = self.shell_status(&shell_request(&format!(
            "mkdir -p {}",
            shell_quote_path(path)
        )))?;
        if status.success() {
            Ok(())
        } else {
            Err(BackendError::Io(std::io::Error::other(format!(
                "ssh mkdir exited with status {status}"
            ))))
        }
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, BackendError> {
        let output = self.shell_output(&shell_request(&format!(
            "cd -P {} && pwd -P",
            shell_quote_path(path)
        )))?;
        successful_output_path(&output)
    }

    fn validate_program(&self, program: &OsStr) -> Result<(), BackendError> {
        let command = if program_is_path(program) {
            format!("test -x {}", shell_quote_os(program))
        } else {
            format!("command -v {} >/dev/null 2>&1", shell_quote_os(program))
        };

        if self.bool_status(&shell_request(&command))? {
            Ok(())
        } else {
            Err(BackendError::MissingProgram(
                program.to_string_lossy().into_owned(),
            ))
        }
    }
}

fn shell_request(command: &str) -> CommandRequest {
    CommandRequest::new("sh").args(["-lc", command])
}

fn successful_output_path(output: &CommandOutput) -> Result<PathBuf, BackendError> {
    if !output.status.success() {
        return Err(BackendError::Io(std::io::Error::other(format!(
            "ssh command exited with status {}",
            output.status
        ))));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
    ))
}

fn program_is_path(program: &OsStr) -> bool {
    let path = Path::new(program);
    path.is_absolute() || path.components().count() > 1
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote_os(path.as_os_str())
}

fn shell_quote_os(value: &OsStr) -> String {
    shell_quote(&value.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
