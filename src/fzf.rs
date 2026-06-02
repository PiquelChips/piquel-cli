use std::{
    io::{self, Write},
    process::{Command, Stdio},
};

#[derive(Debug)]
pub enum FzfError {
    MissingBinary,
    Cancelled,
    Io(io::Error),
    Command(String),
}

impl std::fmt::Display for FzfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FzfError::MissingBinary => write!(f, "fzf is not installed or not available in PATH"),
            FzfError::Cancelled => write!(f, "fzf selection was cancelled"),
            FzfError::Io(e) => write!(f, "IO error: {e}"),
            FzfError::Command(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for FzfError {}

impl From<io::Error> for FzfError {
    fn from(e: io::Error) -> Self {
        FzfError::Io(e)
    }
}

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
    let mut child = Command::new(program)
        .arg("--prompt")
        .arg(prompt)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                FzfError::MissingBinary
            } else {
                FzfError::Io(e)
            }
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn cancelled_selection_returns_none() {
        let fake_fzf = test_script(
            "cancelled-fzf",
            r#"#!/bin/sh
exit 130
"#,
        );

        let selection = select_with_program(
            fake_fzf.to_str().unwrap(),
            vec!["one".to_owned(), "two".to_owned()],
            "piquel> ",
        )
        .unwrap();

        assert_eq!(selection, None);
    }

    fn test_script(name: &str, content: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("piquelcli-{name}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join(name);
        fs::write(&script, content).unwrap();

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }

        script
    }
}
