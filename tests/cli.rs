use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("piquelcli-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).expect("temp dir should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_config(temp: &TestDir, contents: &str) -> PathBuf {
    let path = temp.path().join("config.json");
    fs::write(&path, contents).expect("test config should be written");
    path
}

fn config_with_projects(temp: &TestDir) -> PathBuf {
    write_config(
        temp,
        r#"{
            "projects_dir": "/tmp/projects",
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] },
                "rust": { "windows": [{ "commands": ["cargo check"] }] }
            },
            "projects": [
                {
                    "repository": "git@github.com:owner/zeta.git"
                },
                {
                    "repository": "https://github.com/owner/alpha.git",
                    "default_session": "rust"
                }
            ]
        }"#,
    )
}

fn piquel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_piquelcli"))
}

fn assert_success(output: Output, stdout: &str, stderr: &str) {
    assert!(
        output.status.success(),
        "expected success, got status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), stdout);
    assert_eq!(String::from_utf8_lossy(&output.stderr), stderr);
}

fn assert_failure(output: Output, stderr_contains: &[&str]) {
    assert!(
        !output.status.success(),
        "expected failure, got success\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in stderr_contains {
        assert!(
            stderr.contains(expected),
            "expected stderr to contain {expected:?}\nstderr:\n{stderr}"
        );
    }
}

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    piquel().args(args).output().expect("piquelcli should run")
}

#[test]
fn project_list_prints_sorted_configured_projects() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);

    let output = run(["--config", config.to_str().unwrap(), "project", "list"]);

    assert_success(output, "alpha\nzeta\n", "");
}

#[test]
fn missing_config_file_exits_with_clear_error() {
    let temp = TestDir::new();
    let config = temp.path().join("missing.json");

    let output = run(["--config", config.to_str().unwrap(), "project", "list"]);

    assert_failure(output, &["Config file", "missing.json"]);
}

#[test]
fn invalid_config_schema_exits_with_parse_error() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": {
                    "root": "/tmp",
                    "windows": [{ "commands": [] }]
                }
            }
        }"#,
    );

    let output = run(["--config", config.to_str().unwrap(), "project", "list"]);

    assert_failure(output, &["Failed to parse config", "unknown field `root`"]);
}

#[test]
fn project_load_rejects_missing_project_path_before_opening_tmux_session() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "projects_dir": "/tmp/piquelcli-test-projects-that-do-not-exist",
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": [
                {
                    "repository": "https://github.com/owner/alpha.git"
                }
            ]
        }"#,
    );

    let output = piquel()
        .env_remove("TMUX")
        .args([
            "--config",
            config.to_str().unwrap(),
            "project",
            "load",
            "alpha",
        ])
        .output()
        .expect("piquelcli should run");

    assert_failure(output, &["Project \"alpha\" path", "does not exist"]);
}

#[test]
fn list_prints_sorted_deduplicated_tmux_sessions() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let tmux_bin = temp.path().join("bin");
    fs::create_dir(&tmux_bin).expect("fake tmux bin dir should be created");
    let tmux = tmux_bin.join("tmux");

    fs::write(
        &tmux,
        r#"#!/usr/bin/env sh
if [ "$1" = "list-sessions" ]; then
    printf '%s\n' zeta alpha zeta
    exit 0
fi
exit 64
"#,
    )
    .expect("fake tmux should be written");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755))
            .expect("fake tmux should be executable");
    }

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![tmux_bin];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).expect("test PATH should be valid");

    let output = piquel()
        .env("PATH", path)
        .args(["--config", config.to_str().unwrap(), "list"])
        .output()
        .expect("piquelcli should run");

    assert_success(output, "alpha\nzeta\n", "");
}
