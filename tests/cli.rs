#![allow(missing_docs)]

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

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test paths should be valid UTF-8")
}

fn shell_path() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL").map(PathBuf::from)
        && shell.exists()
    {
        return shell;
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join("sh"))
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("test executable should be written");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("test executable should be executable");
    }
}

fn bin_dir_with(temp: &TestDir, binaries: &[(&str, String)]) -> PathBuf {
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin dir should be created");

    for (name, contents) in binaries {
        write_executable(&bin.join(name), contents);
    }

    bin
}

fn prepend_path(bin: &Path) -> std::ffi::OsString {
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(&old_path));
    std::env::join_paths(paths).expect("test PATH should be valid")
}

fn fake_tmux_script(log: &Path, list_sessions: &str) -> String {
    format!(
        r#"#!{}
log={}
printf '%s\n' "$*" >> "$log"

case "$1" in
    list-sessions)
        printf '{}'
        exit 0
        ;;
    new-session)
        exit 0
        ;;
    list-windows)
        printf 'bootstrap-window\n'
        exit 0
        ;;
    new-window)
        printf 'window-id\n'
        exit 0
        ;;
    send-keys|kill-window|select-window|attach)
        exit 0
        ;;
esac

exit 64
"#,
        path_str(&shell_path()),
        shell_quote(path_str(log)),
        list_sessions
    )
}

fn fake_fzf_script(selection: &str, input_log: &Path) -> String {
    format!(
        r"#!{}
cat > {}
printf '%s\n' {}
",
        path_str(&shell_path()),
        shell_quote(path_str(input_log)),
        shell_quote(selection)
    )
}

fn fake_git_worktree_script(main_path: &Path, branch_path: &Path) -> String {
    format!(
        r#"#!{}
if [ "$1" = "-C" ] && [ "$3" = "worktree" ] && [ "$4" = "list" ] && [ "$5" = "--porcelain" ]; then
    cat <<'EOF'
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree {}
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature/foo

EOF
    exit 0
fi

exit 64
"#,
        path_str(&shell_path()),
        path_str(main_path),
        path_str(branch_path)
    )
}

fn assert_success(output: &Output, stdout: &str, stderr: &str) {
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

fn assert_failure(output: &Output, stderr_contains: &[&str]) {
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

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn missing_config_file_exits_with_clear_error() {
    let temp = TestDir::new();
    let config = temp.path().join("missing.json");

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_failure(&output, &["Config file", "missing.json"]);
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

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_failure(&output, &["Failed to parse config", "unknown field `root`"]);
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
        .args(["--config", path_str(&config), "project", "load", "alpha"])
        .output()
        .expect("piquelcli should run");

    assert_failure(&output, &["Project \"alpha\" path", "does not exist"]);
}

#[test]
fn list_prints_sorted_deduplicated_tmux_sessions() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let tmux_bin = temp.path().join("bin");
    fs::create_dir(&tmux_bin).expect("fake tmux bin dir should be created");
    let tmux = tmux_bin.join("tmux");

    write_executable(
        &tmux,
        &format!(
            r#"#!{}
if [ "$1" = "list-sessions" ]; then
    printf '%s\n' zeta alpha zeta
    exit 0
fi
exit 64
"#,
            path_str(&shell_path())
        ),
    );

    let output = piquel()
        .env("PATH", prepend_path(&tmux_bin))
        .args(["--config", path_str(&config), "list"])
        .output()
        .expect("piquelcli should run");

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn project_load_creates_tmux_session_from_configured_template() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = write_config(
        &temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{
                        "windows": [
                            {{ "commands": ["vim ."] }},
                            {{ "commands": ["cargo test"] }}
                        ]
                    }}
                }},
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git",
                        "path": {}
                    }}
                ]
            }}"#,
            serde_json::to_string(path_str(&project_path)).expect("path should serialize")
        ),
    );
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "project", "load", "alpha"])
        .output()
        .expect("piquelcli should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("list-sessions -F #{session_name}"));
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha",
        path_str(&project_path)
    )));
    assert!(log.contains("new-window -P -F #{window_id} -t alpha"));
    assert!(log.contains("send-keys -t window-id vim . Enter"));
    assert!(log.contains("send-keys -t window-id cargo test Enter"));
    assert!(log.contains("kill-window -t bootstrap-window"));
    assert!(log.contains("select-window -t window-id"));
    assert!(log.contains("attach -t alpha"));
}

#[test]
fn project_load_worktree_opens_requested_branch_worktree() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    let worktree_path = temp.path().join("worktrees/alpha-feature");
    fs::create_dir_all(&project_path).expect("project path should be created");
    fs::create_dir_all(&worktree_path).expect("worktree path should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = write_config(
        &temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{ "windows": [{{ "commands": [] }}] }}
                }},
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git",
                        "path": {}
                    }}
                ]
            }}"#,
            serde_json::to_string(path_str(&project_path)).expect("path should serialize")
        ),
    );
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            (
                "git",
                fake_git_worktree_script(&project_path, &worktree_path),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "project",
            "load",
            "alpha",
            "--worktree",
            "feature/foo",
        ])
        .output()
        .expect("piquelcli should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha--feature_foo",
        path_str(&worktree_path)
    )));
    assert!(log.contains("attach -t alpha--feature_foo"));
}

#[test]
fn pick_routes_fzf_tmux_selection_to_attach() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let tmux_log = temp.path().join("tmux.log");
    let fzf_input = temp.path().join("fzf-input.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "beta\n")),
            ("fzf", fake_fzf_script("beta", &fzf_input)),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick"])
        .output()
        .expect("piquelcli should run");

    assert_success(&output, "", "");

    let fzf_items = fs::read_to_string(fzf_input).expect("fzf input should be readable");
    assert_eq!(fzf_items, "alpha\nbeta\nzeta\n");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("list-sessions -F #{session_name}"));
    assert!(log.contains("attach -t beta"));
}
