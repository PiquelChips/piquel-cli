use std::{
    io::{self, Write},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
use thiserror::Error;

/// Errors produced while running `fzf`.
#[derive(Debug, Error)]
pub enum FzfError {
    /// The `fzf` binary could not be found.
    #[error("fzf is not installed or not available in PATH")]
    MissingBinary,
    /// Selection was cancelled by the user.
    #[error("fzf selection was cancelled")]
    Cancelled,
    /// An IO operation failed while communicating with `fzf`.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// The `fzf` process exited with an unexpected error.
    #[error("{0}")]
    Command(String),
}

/// Presents `items` in `fzf` and returns the selected item, if any.
///
/// # Errors
///
/// Returns an error if `fzf` is missing, cannot be spawned, or exits with an
/// unexpected failure.
pub fn select<I>(items: I, prompt: &str) -> Result<Option<String>, FzfError>
where
    I: IntoIterator<Item = String>,
{
    select_with_program("fzf", items, prompt)
}

fn select_with_program<I>(program: &str, items: I, prompt: &str) -> Result<Option<String>, FzfError>
where
    I: IntoIterator<Item = String>,
{
    let mut child = spawn_fzf(program, prompt)?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| FzfError::Command("Failed to open fzf stdin".to_owned()))?;

        for item in items {
            if let Err(e) = writeln!(stdin, "{item}") {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    break;
                }
                return Err(FzfError::Io(e));
            }
        }
    }

    let output = child.wait_with_output().map_err(FzfError::Io)?;
    let selection = String::from_utf8_lossy(&output.stdout)
        .trim_matches('\n')
        .to_owned();

    if output.status.success() {
        if selection.is_empty() {
            Ok(None)
        } else {
            Ok(Some(selection))
        }
    } else if selection.is_empty() {
        Ok(None)
    } else {
        Err(FzfError::Command(format!(
            "fzf exited with status {}",
            output.status
        )))
    }
}

fn spawn_fzf(program: &str, prompt: &str) -> Result<Child, FzfError> {
    let mut last_error = None;

    for _ in 0..5 {
        match Command::new(program)
            .arg("--prompt")
            .arg(prompt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(error) if is_executable_file_busy(&error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(FzfError::MissingBinary);
            }
            Err(error) => return Err(FzfError::Io(error)),
        }
    }

    match last_error {
        Some(error) => Err(FzfError::Io(error)),
        None => unreachable!("retry loop always runs at least once"),
    }
}

fn is_executable_file_busy(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(26)
    }

    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn cancelled_selection_returns_none() {
        let fake_fzf = test_script(
            "cancelled-fzf",
            r"exit 130
",
        );

        let selection = select_with_program(
            fake_fzf
                .to_str()
                .expect("fake fzf path should be valid UTF-8"),
            vec!["one".to_owned(), "two".to_owned()],
            "piquel> ",
        )
        .expect("fake fzf should run");

        assert_eq!(selection, None);
    }

    #[test]
    fn successful_selection_returns_trimmed_item_and_writes_choices() {
        let dir = test_dir("successful-fzf");
        let input_log = dir.join("input.log");
        let args_log = dir.join("args.log");
        let fake_fzf = test_script_in(
            &dir,
            "fzf",
            &format!(
                r#"for arg in "$@"; do
    printf '%s\n' "$arg"
done > {}
cat > {}
printf 'two\n'
"#,
                shell_quote(&args_log),
                shell_quote(&input_log)
            ),
        );

        let selection = select_with_program(
            fake_fzf
                .to_str()
                .expect("fake fzf path should be valid UTF-8"),
            vec!["one".to_owned(), "two".to_owned()],
            "piquel> ",
        )
        .expect("fake fzf should run");

        assert_eq!(selection, Some("two".to_owned()));
        assert_eq!(
            fs::read_to_string(input_log).expect("input log should be readable"),
            "one\ntwo\n"
        );
        assert_eq!(
            fs::read_to_string(args_log).expect("args log should be readable"),
            "--prompt\npiquel> \n"
        );
    }

    #[test]
    fn successful_empty_selection_returns_none() {
        let fake_fzf = test_script(
            "empty-fzf",
            r"cat >/dev/null
exit 0
",
        );

        let selection = select_with_program(
            fake_fzf
                .to_str()
                .expect("fake fzf path should be valid UTF-8"),
            vec!["one".to_owned()],
            "piquel> ",
        )
        .expect("fake fzf should run");

        assert_eq!(selection, None);
    }

    #[test]
    fn failed_selection_with_stdout_returns_command_error() {
        let fake_fzf = test_script(
            "failed-fzf",
            r"cat >/dev/null
printf 'partial\n'
exit 2
",
        );

        let err = select_with_program(
            fake_fzf
                .to_str()
                .expect("fake fzf path should be valid UTF-8"),
            vec!["one".to_owned()],
            "piquel> ",
        )
        .expect_err("fzf failure should be returned");

        assert!(matches!(err, FzfError::Command(_)));
        assert!(err.to_string().contains("fzf exited with status"));
    }

    #[test]
    fn missing_binary_returns_missing_binary_error() {
        let err = select_with_program(
            "/definitely/missing/piquel/fzf",
            vec!["one".to_owned()],
            "piquel> ",
        )
        .expect_err("missing binary should be returned");

        assert!(matches!(err, FzfError::MissingBinary));
    }

    #[test]
    fn broken_pipe_while_writing_items_is_treated_as_cancelled() {
        let fake_fzf = test_script(
            "early-exit-fzf",
            r"exit 130
",
        );

        let selection = select_with_program(
            fake_fzf
                .to_str()
                .expect("fake fzf path should be valid UTF-8"),
            (0..10_000).map(|index| format!("item-{index}")),
            "piquel> ",
        )
        .expect("broken pipe from early exit should not fail");

        assert_eq!(selection, None);
    }

    fn test_script(name: &str, body: &str) -> PathBuf {
        let dir = test_dir(name);
        test_script_in(&dir, name, body)
    }

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("piquelcli-{name}-{unique}"));
        fs::create_dir_all(&dir).expect("test script directory should be created");
        dir
    }

    fn test_script_in(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let script = dir.join(name);
        let content = format!("#!{}\n{body}", shell_path());
        {
            let mut file = File::create(&script).expect("test script should be created");
            file.write_all(content.as_bytes())
                .expect("test script should be written");
            file.sync_all().expect("test script should be synced");
        }

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&script)
                .expect("test script metadata should be readable")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions)
                .expect("test script permissions should be set");
        }

        script
    }

    fn shell_quote(path: &std::path::Path) -> String {
        format!(
            "'{}'",
            path.to_str()
                .expect("test path should be UTF-8")
                .replace('\'', "'\\''")
        )
    }

    fn shell_path() -> String {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .flat_map(|path| [path.join("sh"), path.join("bash")])
            .find(|path| path.exists())
            .expect("test shell should be available in PATH")
            .to_string_lossy()
            .into_owned()
    }
}
