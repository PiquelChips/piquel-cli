#![allow(missing_docs)]

use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs::{self, File},
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("piquel-test-{}-{id}", std::process::id()));
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
    Command::new(env!("CARGO_BIN_EXE_piquel"))
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test paths should be valid UTF-8")
}

fn shell_path() -> PathBuf {
    let system_sh = PathBuf::from("/bin/sh");
    if system_sh.exists() {
        return system_sh;
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .flat_map(|path| [path.join("sh"), path.join("bash")])
        .find(|path| path.exists())
        .expect("test shell should be available in PATH")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_executable(path: &Path, contents: &str) {
    {
        let mut file = File::create(path).expect("test executable should be created");
        file.write_all(contents.as_bytes())
            .expect("test executable should be written");
        file.sync_all().expect("test executable should be synced");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("test executable should be executable");
    }

    thread::sleep(Duration::from_millis(10));
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

fn fake_tmux_argv_script(log: &Path) -> String {
    format!(
        r#"#!{}
log={}
printf 'argc=%s\n' "$#" >> "$log"
for arg in "$@"; do
    printf '[%s]\n' "$arg" >> "$log"
done

case "$1" in
    list-sessions)
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
        shell_quote(path_str(log))
    )
}

fn machine_config(address: &str) -> String {
    format!(
        r#""machines": [
                {{
                    "name": "pi",
                    "address": {},
                    "username": "ronan"
                }}
            ],"#,
        serde_json::to_string(address).expect("address should serialize")
    )
}

fn fake_ssh_script(log: &Path, list_sessions: &str) -> String {
    format!(
        r#"#!{}
log={}
remote_bin=$(dirname "$0")
PATH="$remote_bin:$PATH"
export PATH
tty_flag=$1
shift
if [ "$1" = "--" ]; then
    shift
fi
target=$1
cmd=$2
printf '%s	%s	%s\n' "$tty_flag" "$target" "$cmd" >> "$log"

case "$cmd" in
    *"printf "*"HOME"*)
        printf '/home/pi\n'
        exit 0
        ;;
    *"pwd -P"*)
        printf '/home/pi\n'
        exit 0
        ;;
    *"command -v "*)
        exit 0
        ;;
    *"test -e "*|*"test -d "*|*"mkdir -p "*)
        exit 0
        ;;
esac

case "$cmd" in
    *"'tmux' 'list-sessions'"*)
        printf '{}'
        exit 0
        ;;
    *"'tmux' 'new-session'"*)
        exit 0
        ;;
    *"'tmux' 'list-windows'"*)
        printf 'bootstrap-window\n'
        exit 0
        ;;
    *"'tmux' 'new-window'"*)
        printf 'window-id\n'
        exit 0
        ;;
    *"'tmux' 'send-keys'"*|*"'tmux' 'kill-window'"*|*"'tmux' 'select-window'"*|*"'tmux' 'attach'"*)
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

fn fake_ssh_eval_script(log: &Path) -> String {
    format!(
        r#"#!{}
log={}
remote_bin=$(dirname "$0")
PATH="$remote_bin:$PATH"
export PATH
tty_flag=$1
shift
if [ "$1" = "--" ]; then
    shift
fi
target=$1
cmd=$2
printf 'tty=%s\ntarget=%s\ncmd=%s\n' "$tty_flag" "$target" "$cmd" >> "$log"
sh -c "$cmd"
"#,
        path_str(&shell_path()),
        shell_quote(path_str(log))
    )
}

fn fake_remote_sh_script() -> String {
    format!(
        r#"#!{}
if [ "$1" = "-lc" ]; then
    shift
    exec {} -c "$@"
fi
exec {} "$@"
"#,
        path_str(&shell_path()),
        shell_quote(path_str(&shell_path())),
        shell_quote(path_str(&shell_path()))
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

fn fake_fzf_sequence_script(selections: &[&str], state: &Path, input_prefix: &Path) -> String {
    let mut cases = String::new();
    for (index, selection) in selections.iter().enumerate() {
        writeln!(
            cases,
            "{}) printf '%s\\n' {} ;;",
            index + 1,
            shell_quote(selection)
        )
        .expect("writing to string should succeed");
    }

    format!(
        r#"#!{}
state={}
prefix={}
count=0
if [ -f "$state" ]; then
    count=$(cat "$state")
fi
next=$((count + 1))
printf '%s\n' "$next" > "$state"
cat > "$prefix.$next"
case "$next" in
{}    *) exit 130 ;;
esac
"#,
        path_str(&shell_path()),
        shell_quote(path_str(state)),
        shell_quote(path_str(input_prefix)),
        cases
    )
}

fn fake_git_script(branches: &str, worktrees: &str, add_log: Option<&Path>) -> String {
    let add_log = add_log.map_or_else(
        || ":".to_owned(),
        |path| {
            format!(
                "log={}\nprintf '%s\\n' \"$*\" >> \"$log\"",
                shell_quote(path_str(path))
            )
        },
    );

    format!(
        r#"#!{}
if [ "$1" = "-C" ] && [ "$3" = "for-each-ref" ]; then
    cat <<'EOF'
{}EOF
    exit 0
fi

if [ "$1" = "-C" ] && [ "$3" = "worktree" ] && [ "$4" = "list" ] && [ "$5" = "--porcelain" ]; then
    cat <<'EOF'
{}EOF
    exit 0
fi

if [ "$1" = "-C" ] && [ "$3" = "worktree" ] && [ "$4" = "add" ]; then
    {}
    exit 0
fi

exit 64
"#,
        path_str(&shell_path()),
        branches,
        worktrees,
        add_log
    )
}

fn fake_git_worktree_script(main_path: &Path, branch_path: &Path) -> String {
    fake_git_script(
        "main\nfeature/foo\n",
        &format!(
            "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree {}
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature/foo

",
            path_str(main_path),
            path_str(branch_path)
        ),
        None,
    )
}

fn fake_git_clone_script(log: &Path) -> String {
    format!(
        r#"#!{}
log={}
if [ "$1" = "clone" ]; then
    printf '%s\n' "$*" >> "$log"
    mkdir -p "$3"
    exit 0
fi

exit 64
"#,
        path_str(&shell_path()),
        shell_quote(path_str(log))
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
    piquel().args(args).output().expect("piquel should run")
}

fn config_for_alpha_project(
    temp: &TestDir,
    project_path: &Path,
    worktrees_dir: Option<&Path>,
) -> PathBuf {
    let worktrees_dir_field = worktrees_dir.map_or_else(String::new, |path| {
        format!(
            r#""worktrees_dir": {},
            "#,
            serde_json::to_string(path_str(path)).expect("path should serialize")
        )
    });

    write_config(
        temp,
        &format!(
            r#"{{
                "projects_dir": "/tmp/projects",
                {}"default_session": "default",
                "sessions": {{
                    "default": {{ "windows": [{{ "commands": ["default cmd"] }}] }},
                    "rust": {{ "windows": [{{ "commands": ["cargo check"] }}] }}
                }},
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git",
                        "path": {}
                    }}
                ]
            }}"#,
            worktrees_dir_field,
            serde_json::to_string(path_str(project_path)).expect("path should serialize")
        ),
    )
}

fn config_for_session_commands(temp: &TestDir, commands: &[&str]) -> PathBuf {
    let commands = commands
        .iter()
        .map(|command| serde_json::to_string(command).expect("command should serialize"))
        .collect::<Vec<_>>()
        .join(", ");

    write_config(
        temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{ "windows": [{{ "commands": [{commands}] }}] }},
                    "alt": {{ "windows": [{{ "name": "alt-window", "commands": ["alt cmd"] }}] }}
                }},
                "projects": []
            }}"#
        ),
    )
}

#[test]
fn project_list_prints_sorted_configured_projects() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn config_path_can_come_from_environment() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);

    let output = piquel()
        .env("PIQUEL_CONFIG", path_str(&config))
        .args(["project", "list"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn config_flag_takes_precedence_over_environment() {
    let temp = TestDir::new();
    let env_config = temp.path().join("env-config.json");
    fs::write(
        &env_config,
        r#"{
            "projects_dir": "/tmp/projects",
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": [
                {
                    "repository": "https://github.com/owner/env-project.git"
                }
            ]
        }"#,
    )
    .expect("env config should be written");
    let flag_config = config_with_projects(&temp);

    let output = piquel()
        .env("PIQUEL_CONFIG", path_str(&env_config))
        .args(["--config", path_str(&flag_config), "project", "list"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn project_list_with_machine_stays_local() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{ "windows": [{{ "commands": [] }}] }}
                }},
                {}
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git"
                    }}
                ]
            }}"#,
            machine_config("pi.example.test")
        ),
    );
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(&temp, &[("ssh", fake_ssh_script(&ssh_log, ""))]);

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "--machine",
            "pi",
            "project",
            "list",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "alpha\n", "");
    assert!(!ssh_log.exists(), "project list should not invoke ssh");
}

#[test]
fn project_list_with_unknown_machine_fails_before_running_ssh() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(&temp, &[("ssh", fake_ssh_script(&ssh_log, ""))]);

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "--machine",
            "pi",
            "project",
            "list",
        ])
        .output()
        .expect("piquel should run");

    assert_failure(&output, &["Machine \"pi\" is not configured"]);
    assert!(!ssh_log.exists(), "ssh should not be invoked");
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
fn unsafe_project_name_exits_with_validation_error() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "projects_dir": "/tmp/projects",
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": [
                {
                    "repository": "https://github.com/owner/alpha.git",
                    "name": "../alpha"
                }
            ]
        }"#,
    );

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_failure(&output, &["\"../alpha\" is not a valid project name"]);
}

#[test]
fn duplicate_project_names_exit_with_validation_error() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "projects_dir": "/tmp/projects",
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": [
                { "repository": "https://github.com/owner/alpha.git" },
                { "repository": "git@github.com:other/alpha.git" }
            ]
        }"#,
    );

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_failure(&output, &["Duplicate project name \"alpha\""]);
}

#[test]
fn unknown_default_session_exits_with_validation_error() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "projects_dir": "/tmp/projects",
            "default_session": "missing",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": []
        }"#,
    );

    let output = run(["--config", path_str(&config), "project", "list"]);

    assert_failure(
        &output,
        &["Default session template \"missing\" does not exist"],
    );
}

#[test]
fn project_load_rejects_missing_project_path_before_opening_tmux_session() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "projects_dir": "/tmp/piquel-test-projects-that-do-not-exist",
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
    let tmux_log = temp.path().join("tmux.log");
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let mut child = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "project", "load", "alpha"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("piquel should spawn");
    std::io::Write::write_all(child.stdin.as_mut().expect("stdin should be piped"), b"n\n")
        .expect("stdin should be writable");
    let output = child
        .wait_with_output()
        .expect("piquel output should be readable");

    assert_failure(&output, &["Clone", "cancelled"]);
    assert!(!tmux_log.exists(), "tmux should not be opened");
}

#[test]
fn project_load_clones_missing_project_then_opens_tmux_session() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    let tmux_log = temp.path().join("tmux.log");
    let git_log = temp.path().join("git.log");
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
            ("git", fake_git_clone_script(&git_log)),
        ],
    );

    let mut child = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "project", "load", "alpha"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("piquel should spawn");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin should be piped"),
        b"yes\n",
    )
    .expect("stdin should be writable");
    let output = child
        .wait_with_output()
        .expect("piquel output should be readable");

    assert_success(
        &output,
        "",
        &format!(
            "Project \"alpha\" path {} does not exist. Clone https://github.com/owner/alpha.git there? [y/N] ",
            project_path.display()
        ),
    );

    let git_log = fs::read_to_string(git_log).expect("git log should be readable");
    assert_eq!(
        git_log,
        format!(
            "clone https://github.com/owner/alpha.git {}\n",
            path_str(&project_path)
        )
    );

    let tmux_log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(tmux_log.contains(&format!(
        "new-session -d -c {} -s alpha",
        path_str(&project_path)
    )));
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
        .expect("piquel should run");

    assert_success(&output, "alpha\nzeta\n", "");
}

#[test]
fn list_without_machine_stays_local_even_when_machines_are_configured() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{ "windows": [{{ "commands": [] }}] }}
                }},
                {}
                "projects": []
            }}"#,
            machine_config("pi.example.test")
        ),
    );
    let tmux_log = temp.path().join("tmux.log");
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "zeta\nalpha\nzeta\n")),
            ("ssh", fake_ssh_script(&ssh_log, "")),
        ],
    );

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "list"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "alpha\nzeta\n", "");
    assert!(tmux_log.exists(), "tmux should run locally");
    assert!(!ssh_log.exists(), "ssh should not be invoked without -m");
}

#[test]
fn list_with_machine_runs_tmux_over_ssh() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "machines": [
                {
                    "name": "pi",
                    "address": "pi.example.test",
                    "username": "ronan"
                }
            ]
        }"#,
    );
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(
        &temp,
        &[("ssh", fake_ssh_script(&ssh_log, "zeta\nalpha\nzeta\n"))],
    );

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "--machine", "pi", "list"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "alpha\nzeta\n", "");

    let log = fs::read_to_string(ssh_log).expect("ssh log should be readable");
    assert!(log.contains("ronan@pi.example.test"));
    assert!(
        log.contains("-T\tronan@pi.example.test\t'tmux' 'list-sessions' '-F' '#{session_name}'")
    );
    assert!(!log.contains("-t\tronan@pi.example.test\t'tmux' 'list-sessions'"));
}

#[test]
fn machine_ssh_destination_is_terminated_before_remote_command() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "machines": [
                {
                    "name": "pi",
                    "address": "pi.local",
                    "username": "-oProxyCommand=bad"
                }
            ]
        }"#,
    );
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("ssh", fake_ssh_eval_script(&ssh_log)),
            ("sh", fake_remote_sh_script()),
            ("tmux", fake_tmux_script(&temp.path().join("tmux.log"), "")),
        ],
    );

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "--machine", "pi", "list"])
        .output()
        .expect("piquel should run");

    let log = fs::read_to_string(ssh_log).expect("ssh log should be readable");
    assert!(
        output.status.success(),
        "expected success, got status {}\nstdout:\n{}\nstderr:\n{}\nssh log:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        log
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "ssh log:\n{log}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");

    assert!(log.contains("tty=-T\n"));
    assert!(log.contains("target=-oProxyCommand=bad@pi.local\n"));
    assert!(log.contains("cmd='tmux' 'list-sessions' '-F' '#{session_name}'"));
}

#[test]
fn unknown_machine_exits_before_running_ssh() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(&temp, &[("ssh", fake_ssh_script(&ssh_log, ""))]);

    let output = piquel()
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "--machine", "pi", "list"])
        .output()
        .expect("piquel should run");

    assert_failure(&output, &["Machine \"pi\" is not configured"]);
    assert!(!ssh_log.exists(), "ssh should not be invoked");
}

#[test]
fn project_load_with_machine_opens_remote_tmux_session() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": {
                    "windows": [
                        { "name": "editor", "commands": ["vim ."] }
                    ]
                }
            },
            "projects": [
                {
                    "repository": "https://github.com/owner/alpha.git",
                    "path": "/srv/projects/alpha"
                }
            ],
            "machines": [
                {
                    "name": "pi",
                    "address": "pi.local",
                    "username": "ronan"
                }
            ]
        }"#,
    );
    let ssh_log = temp.path().join("ssh.log");
    let bin = bin_dir_with(&temp, &[("ssh", fake_ssh_script(&ssh_log, ""))]);

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "-m",
            "pi",
            "project",
            "load",
            "alpha",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(ssh_log).expect("ssh log should be readable");
    assert!(log.contains("ronan@pi.local"));
    assert!(log.contains(
        "-T\tronan@pi.local\t'tmux' 'new-session' '-d' '-c' '/srv/projects/alpha' '-s' 'alpha'"
    ));
    assert!(log.contains("'tmux' 'new-window' '-P' '-F' '#{window_id}' '-n' 'editor' '-t' 'alpha' '-c' '/srv/projects/alpha'"));
    assert!(log.contains("'tmux' 'send-keys' '-t' 'window-id' 'vim .' 'Enter'"));
    assert!(log.contains("-t\tronan@pi.local\t'tmux' 'attach' '-t' 'alpha'"));
}

#[test]
fn project_load_with_machine_shell_quotes_remote_tmux_arguments() {
    let temp = TestDir::new();
    let project_path = temp.path().join("remote project's dir");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let pwned = temp.path().join("shell-was-not-quoted");
    let command = format!("printf hacked > {}; echo done", path_str(&pwned));
    let config = write_config(
        &temp,
        &format!(
            r#"{{
                "default_session": "default",
                "sessions": {{
                    "default": {{
                        "windows": [
                            {{ "name": "editor's window", "commands": [{}] }}
                        ]
                    }}
                }},
                {}
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git",
                        "path": {}
                    }}
                ]
            }}"#,
            serde_json::to_string(&command).expect("command should serialize"),
            machine_config("pi.local"),
            serde_json::to_string(path_str(&project_path)).expect("path should serialize")
        ),
    );
    let ssh_log = temp.path().join("ssh.log");
    let tmux_log = temp.path().join("tmux.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("ssh", fake_ssh_eval_script(&ssh_log)),
            ("sh", fake_remote_sh_script()),
            ("tmux", fake_tmux_argv_script(&tmux_log)),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "-m",
            "pi",
            "project",
            "load",
            "alpha",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");
    assert!(
        !pwned.exists(),
        "configured tmux command should be passed as an argument, not executed by the remote shell"
    );

    let tmux_log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(tmux_log.contains(&format!("[{}]", path_str(&project_path))));
    assert!(tmux_log.contains("[editor's window]"));
    assert!(tmux_log.contains(&format!("[{command}]")));

    let ssh_log = fs::read_to_string(ssh_log).expect("ssh log should be readable");
    assert!(ssh_log.contains("remote project'\\''s dir'"));
    assert!(ssh_log.contains("'editor'\\''s window'"));
    assert!(ssh_log.contains("'printf hacked > "));
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
        .expect("piquel should run");

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
fn project_load_uses_project_inline_default_session() {
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
                    "default": {{ "windows": [{{ "commands": ["global cmd"] }}] }}
                }},
                "projects": [
                    {{
                        "repository": "https://github.com/owner/alpha.git",
                        "path": {},
                        "default_session": {{
                            "windows": [
                                {{ "name": "inline", "commands": ["inline cmd"] }}
                            ]
                        }}
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
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("new-window -P -F #{window_id} -n inline -t alpha"));
    assert!(log.contains("send-keys -t window-id inline cmd Enter"));
    assert!(!log.contains("global cmd"));
}

#[test]
fn project_load_session_override_uses_named_template_over_project_default() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "project",
            "load",
            "alpha",
            "--session",
            "rust",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("send-keys -t window-id cargo check Enter"));
    assert!(!log.contains("send-keys -t window-id default cmd Enter"));
}

#[test]
fn project_load_unknown_session_override_exits_before_tmux() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);

    let output = piquel()
        .env_remove("TMUX")
        .args([
            "--config",
            path_str(&config),
            "project",
            "load",
            "alpha",
            "--session",
            "missing",
        ])
        .output()
        .expect("piquel should run");

    assert_failure(&output, &["Session template \"missing\" is not configured"]);
}

#[test]
fn session_dot_path_uses_current_directory_name() {
    let temp = TestDir::new();
    let session_path = temp.path().join("workspaces/alpha");
    fs::create_dir_all(&session_path).expect("session path should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": ["vim ."] }] }
            }
        }"#,
    );
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let output = piquel()
        .current_dir(&session_path)
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "session", "."])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let resolved_session_path = session_path
        .canonicalize()
        .expect("session path should resolve");
    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha",
        path_str(&resolved_session_path)
    )));
    assert!(log.contains("send-keys -t window-id vim . Enter"));
    assert!(log.contains("attach -t alpha"));
}

#[test]
fn project_load_names_configured_tmux_windows() {
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
                            {{ "name": "editor", "commands": ["vim ."] }}
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
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("new-window -P -F #{window_id} -n editor -t alpha"));
    assert!(log.contains("send-keys -t window-id vim . Enter"));
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
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha--feature_foo",
        path_str(&worktree_path)
    )));
    assert!(log.contains("attach -t alpha--feature_foo"));
}

#[test]
fn project_load_worktree_rejects_non_local_branch() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let bin = bin_dir_with(
        &temp,
        &[(
            "git",
            fake_git_script(
                "main\n",
                &format!(
                    "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                    path_str(&project_path)
                ),
                None,
            ),
        )],
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
            "feature/missing",
        ])
        .output()
        .expect("piquel should run");

    assert_failure(
        &output,
        &["Branch \"feature/missing\" is not a local branch for project \"alpha\""],
    );
}

#[test]
fn project_load_worktree_creates_missing_managed_worktree() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    let worktrees_dir = temp.path().join("managed-worktrees");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, Some(&worktrees_dir));
    let tmux_log = temp.path().join("tmux.log");
    let git_add_log = temp.path().join("git-add.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            (
                "git",
                fake_git_script(
                    "main\nfeature/foo\n",
                    &format!(
                        "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                        path_str(&project_path)
                    ),
                    Some(&git_add_log),
                ),
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
        .expect("piquel should run");

    assert_success(&output, "", "");

    let managed_path = worktrees_dir.join("alpha/feature_foo");
    let git_log = fs::read_to_string(git_add_log).expect("git add log should be readable");
    assert!(git_log.contains(&format!(
        "-C {} worktree add {} feature/foo",
        path_str(&project_path),
        path_str(&managed_path)
    )));

    let tmux_log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(tmux_log.contains(&format!(
        "new-session -d -c {} -s alpha--feature_foo",
        path_str(&managed_path)
    )));
}

#[test]
fn project_load_inside_tmux_is_rejected_before_project_lookup() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);

    let output = piquel()
        .env("TMUX", "/tmp/tmux")
        .args(["--config", path_str(&config), "project", "load", "alpha"])
        .output()
        .expect("piquel should run");

    assert_failure(&output, &["Please do not use this command in tmux"]);
}

#[test]
fn session_opens_directory_with_default_template_and_name_override() {
    let temp = TestDir::new();
    let root = temp.path().join("workspace dir");
    fs::create_dir(&root).expect("session root should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = config_for_session_commands(&temp, &["echo default"]);
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "session",
            path_str(&root),
            "--name",
            "custom:name",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s custom_name",
        path_str(&root)
    )));
    assert!(log.contains("send-keys -t window-id echo default Enter"));
}

#[test]
fn session_alias_uses_requested_template_and_derived_directory_name() {
    let temp = TestDir::new();
    let root = temp.path().join("workspace");
    fs::create_dir(&root).expect("session root should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = config_for_session_commands(&temp, &["echo default"]);
    let bin = bin_dir_with(&temp, &[("tmux", fake_tmux_script(&tmux_log, ""))]);

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "s",
            path_str(&root),
            "--session",
            "alt",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s workspace",
        path_str(&root)
    )));
    assert!(log.contains("new-window -P -F #{window_id} -n alt-window -t workspace"));
    assert!(log.contains("send-keys -t window-id alt cmd Enter"));
    assert!(!log.contains("echo default"));
}

#[test]
fn session_existing_tmux_session_attaches_without_creating_windows() {
    let temp = TestDir::new();
    let root = temp.path().join("workspace");
    fs::create_dir(&root).expect("session root should be created");
    let tmux_log = temp.path().join("tmux.log");
    let config = config_for_session_commands(&temp, &["echo default"]);
    let bin = bin_dir_with(
        &temp,
        &[("tmux", fake_tmux_script(&tmux_log, "workspace\n"))],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "session", path_str(&root)])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("attach -t workspace"));
    assert!(!log.contains("new-session"));
    assert!(!log.contains("new-window"));
}

#[test]
fn session_rejects_missing_path_file_path_and_unknown_template() {
    let temp = TestDir::new();
    let config = config_for_session_commands(&temp, &["echo default"]);
    let file_path = temp.path().join("file.txt");
    fs::write(&file_path, "not a directory").expect("test file should be written");
    let missing_path = temp.path().join("missing");

    let missing_output = piquel()
        .env_remove("TMUX")
        .args([
            "--config",
            path_str(&config),
            "session",
            path_str(&missing_path),
        ])
        .output()
        .expect("piquel should run");
    assert_failure(&missing_output, &["Session path", "does not exist"]);

    let file_output = piquel()
        .env_remove("TMUX")
        .args([
            "--config",
            path_str(&config),
            "session",
            path_str(&file_path),
        ])
        .output()
        .expect("piquel should run");
    assert_failure(&file_output, &["Session path", "is not a directory"]);

    let template_output = piquel()
        .env_remove("TMUX")
        .args([
            "--config",
            path_str(&config),
            "session",
            path_str(temp.path()),
            "--session",
            "missing",
        ])
        .output()
        .expect("piquel should run");
    assert_failure(
        &template_output,
        &["Session template \"missing\" is not configured"],
    );
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
        .expect("piquel should run");

    assert_success(&output, "", "");

    let fzf_items = fs::read_to_string(fzf_input).expect("fzf input should be readable");
    assert_eq!(fzf_items, "alpha\nbeta\nzeta\n");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("list-sessions -F #{session_name}"));
    assert!(log.contains("attach -t beta"));
}

#[test]
fn pick_cancelled_selection_exits_successfully_without_attach() {
    let temp = TestDir::new();
    let config = config_with_projects(&temp);
    let tmux_log = temp.path().join("tmux.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "beta\n")),
            (
                "fzf",
                format!(
                    r"#!{}
cat >/dev/null
exit 130
",
                    path_str(&shell_path())
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("list-sessions -F #{session_name}"));
    assert!(!log.contains("attach -t beta"));
}

#[test]
fn pick_without_choices_exits_successfully_without_fzf() {
    let temp = TestDir::new();
    let config = write_config(
        &temp,
        r#"{
            "default_session": "default",
            "sessions": {
                "default": { "windows": [{ "commands": [] }] }
            },
            "projects": []
        }"#,
    );
    let tmux_log = temp.path().join("tmux.log");
    let fzf_log = temp.path().join("fzf.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            (
                "fzf",
                format!(
                    r"#!{}
touch {}
exit 1
",
                    path_str(&shell_path()),
                    shell_quote(path_str(&fzf_log))
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("list-sessions -F #{session_name}"));
    assert!(!fzf_log.exists(), "fzf should not run without choices");
}

#[test]
fn pick_with_session_still_attaches_selected_tmux_session_unchanged() {
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
        .args(["--config", path_str(&config), "pick", "--session", "rust"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("attach -t beta"));
    assert!(!log.contains("cargo check"));
}

#[test]
fn pick_project_argument_skips_first_picker_and_shows_branch_picker() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    let worktree_path = temp.path().join("worktrees/alpha-feature");
    fs::create_dir_all(&project_path).expect("project path should be created");
    fs::create_dir_all(&worktree_path).expect("worktree path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let tmux_log = temp.path().join("tmux.log");
    let fzf_input = temp.path().join("fzf-input.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            ("fzf", fake_fzf_script("feature/foo", &fzf_input)),
            (
                "git",
                fake_git_worktree_script(&project_path, &worktree_path),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick", "alpha"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let fzf_items = fs::read_to_string(fzf_input).expect("fzf input should be readable");
    assert_eq!(fzf_items, "feature/foo\nmain\n");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha--feature_foo",
        path_str(&worktree_path)
    )));
}

#[test]
fn pick_project_branch_checked_out_at_project_path_uses_branch_session_name() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let tmux_log = temp.path().join("tmux.log");
    let fzf_input = temp.path().join("fzf-input.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            ("fzf", fake_fzf_script("main", &fzf_input)),
            (
                "git",
                fake_git_script(
                    "main\n",
                    &format!(
                        "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                        path_str(&project_path)
                    ),
                    None,
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick", "alpha"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha--main",
        path_str(&project_path)
    )));
}

#[test]
fn pick_project_with_no_local_branches_opens_project_session() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let tmux_log = temp.path().join("tmux.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            ("git", fake_git_script("", "", None)),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick", "alpha"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains(&format!(
        "new-session -d -c {} -s alpha",
        path_str(&project_path)
    )));
}

#[test]
fn pick_project_missing_branch_worktree_runs_git_worktree_add() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    let worktrees_dir = temp.path().join("managed-worktrees");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, Some(&worktrees_dir));
    let tmux_log = temp.path().join("tmux.log");
    let fzf_input = temp.path().join("fzf-input.log");
    let git_add_log = temp.path().join("git-add.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            ("fzf", fake_fzf_script("feature/foo", &fzf_input)),
            (
                "git",
                fake_git_script(
                    "main\nfeature/foo\n",
                    &format!(
                        "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                        path_str(&project_path)
                    ),
                    Some(&git_add_log),
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick", "alpha"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let managed_path = worktrees_dir.join("alpha/feature_foo");
    let git_log = fs::read_to_string(git_add_log).expect("git add log should be readable");
    assert!(git_log.contains(&format!(
        "-C {} worktree add {} feature/foo",
        path_str(&project_path),
        path_str(&managed_path)
    )));
}

#[test]
fn pick_session_override_applies_to_project_created_session() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let tmux_log = temp.path().join("tmux.log");
    let fzf_input = temp.path().join("fzf-input.log");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            ("fzf", fake_fzf_script("main", &fzf_input)),
            (
                "git",
                fake_git_script(
                    "main\n",
                    &format!(
                        "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                        path_str(&project_path)
                    ),
                    None,
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args([
            "--config",
            path_str(&config),
            "pick",
            "alpha",
            "--session",
            "rust",
        ])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("send-keys -t window-id cargo check Enter"));
    assert!(!log.contains("send-keys -t window-id default cmd Enter"));
}

#[test]
fn pick_session_override_applies_after_selecting_project_from_first_picker() {
    let temp = TestDir::new();
    let project_path = temp.path().join("projects/alpha");
    fs::create_dir_all(&project_path).expect("project path should be created");
    let config = config_for_alpha_project(&temp, &project_path, None);
    let tmux_log = temp.path().join("tmux.log");
    let fzf_state = temp.path().join("fzf-state");
    let fzf_prefix = temp.path().join("fzf-input");
    let bin = bin_dir_with(
        &temp,
        &[
            ("tmux", fake_tmux_script(&tmux_log, "")),
            (
                "fzf",
                fake_fzf_sequence_script(&["alpha", "main"], &fzf_state, &fzf_prefix),
            ),
            (
                "git",
                fake_git_script(
                    "main\n",
                    &format!(
                        "\
worktree {}
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

",
                        path_str(&project_path)
                    ),
                    None,
                ),
            ),
        ],
    );

    let output = piquel()
        .env_remove("TMUX")
        .env("PATH", prepend_path(&bin))
        .args(["--config", path_str(&config), "pick", "--session", "rust"])
        .output()
        .expect("piquel should run");

    assert_success(&output, "", "");

    let first_input =
        fs::read_to_string(fzf_prefix.with_extension("1")).expect("first fzf input readable");
    let second_input =
        fs::read_to_string(fzf_prefix.with_extension("2")).expect("second fzf input readable");
    assert_eq!(first_input, "alpha\n");
    assert_eq!(second_input, "main\n");

    let log = fs::read_to_string(tmux_log).expect("tmux log should be readable");
    assert!(log.contains("send-keys -t window-id cargo check Enter"));
    assert!(!log.contains("send-keys -t window-id default cmd Enter"));
}
