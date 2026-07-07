use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

use crate::backend::{Backend, BackendError, CommandInput, CommandOutput, CommandRequest};

/// Local process-backed backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalBackend;

impl Backend for LocalBackend {
    fn output(&self, request: CommandRequest) -> Result<CommandOutput, BackendError> {
        let stdin = request.stdin_config().clone();
        self.validate_program(request.program())?;
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

    fn status(&self, request: CommandRequest) -> Result<ExitStatus, BackendError> {
        let stdin = request.stdin_config().clone();
        self.validate_program(request.program())?;
        let mut child = Command::from(request)
            .stdin(Stdio::from(stdin.clone()))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        write_stdin(&mut child, &stdin)?;

        child.wait().map_err(BackendError::Io)
    }

    fn home_dir(&self) -> Result<PathBuf, BackendError> {
        env::home_dir().ok_or(BackendError::HomeDirNotFound)
    }

    fn current_dir(&self) -> Result<PathBuf, BackendError> {
        env::current_dir().map_err(BackendError::Io)
    }

    fn path_exists(&self, path: &Path) -> Result<bool, BackendError> {
        Ok(path.exists())
    }

    fn path_is_dir(&self, path: &Path) -> Result<bool, BackendError> {
        Ok(path.is_dir())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), BackendError> {
        fs::create_dir_all(path).map_err(BackendError::Io)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, BackendError> {
        path.canonicalize().map_err(BackendError::Io)
    }

    fn validate_program(&self, program: &OsStr) -> Result<(), BackendError> {
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

        Err(BackendError::MissingProgram(
            program.to_string_lossy().into_owned(),
        ))
    }
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

fn write_stdin(child: &mut std::process::Child, stdin: &CommandInput) -> Result<(), BackendError> {
    if let CommandInput::Bytes(input) = stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        match child_stdin.write_all(input) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
            Err(error) => return Err(BackendError::Io(error)),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::shell;
    use super::*;

    #[test]
    fn local_backend_pipes_bytes_to_output_command() {
        let output = LocalBackend
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
    fn local_backend_status_reports_exit_code() {
        let status = LocalBackend
            .status(CommandRequest::new(shell()).args(["-c", "exit 7"]))
            .expect("shell command should run");

        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn local_backend_validates_path_programs() {
        LocalBackend
            .validate_program(&shell())
            .expect("shell path should validate");
    }

    #[test]
    fn local_backend_rejects_missing_programs() {
        let err = LocalBackend
            .validate_program(OsStr::new("piquel-definitely-missing-program"))
            .expect_err("missing program should be rejected");

        assert!(matches!(err, BackendError::MissingProgram(_)));
    }
}
