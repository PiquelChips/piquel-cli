use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    command::{self, CommandError, CommandOutput, CommandRequest},
    git::{self, Worktree},
};

/// Local state for managed SSHFS mounts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountState {
    /// Known managed mounts.
    pub mounts: Vec<MountRecord>,
}

/// A single managed mount record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountRecord {
    /// Configured machine name.
    pub machine: String,
    /// Configured project name.
    pub project: String,
    /// Mounted workspace identity.
    pub workspace: MountedWorkspace,
    /// SSH/SFTP target.
    pub target: SshTarget,
    /// Remote source path.
    pub remote_path: PathBuf,
    /// Local SSHFS mount path.
    pub mount_path: PathBuf,
}

/// Workspace kind represented by a managed mount.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MountedWorkspace {
    /// The configured project checkout.
    Main,
    /// An existing git worktree for a branch.
    Branch {
        /// Branch name.
        branch: String,
    },
    /// A detached worktree.
    Detached {
        /// Human-readable detached label.
        label: String,
    },
}

/// SSH target for an SSHFS mount.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshTarget {
    /// SSH username.
    pub username: String,
    /// SSH host or address.
    pub host: String,
}

/// Managed mount plus current OS mount-table status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMount {
    /// Stored mount record.
    pub record: MountRecord,
    /// Current status.
    pub status: MountStatus,
}

/// Current status of a known mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountStatus {
    /// Present in state but not currently mounted.
    Recorded,
    /// Present in state and listed by the OS mount table.
    Mounted,
    /// Present in state but the mountpoint path no longer exists.
    Missing,
}

/// Errors produced while managing SSHFS mounts.
#[derive(Debug, Error)]
pub enum MountError {
    /// Local command or program lookup failed.
    #[error("{0}")]
    CommandExec(#[from] CommandError),
    /// Local state IO failed.
    #[error("Mount state IO error: {0}")]
    Io(#[from] io::Error),
    /// Local state JSON could not be parsed or written.
    #[error("Mount state JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Remote git discovery failed.
    #[error("{0}")]
    Git(#[from] git::GitError),
    /// Another piquel process holds the state lock.
    #[error("Mount state is locked at {}; another piquel mount command may be running", .0.display())]
    StateLocked(PathBuf),
    /// A command exited unsuccessfully.
    #[error("{0}")]
    Command(String),
}

/// Mounts `record` with SSHFS.
///
/// # Errors
///
/// Returns an error if `sshfs` is missing, the mountpoint cannot be created,
/// or `sshfs` exits unsuccessfully.
pub fn mount_workspace(record: &MountRecord) -> Result<(), MountError> {
    command::validate_program(std::ffi::OsStr::new("sshfs"))?;
    fs::create_dir_all(&record.mount_path)?;
    let output = command::output(sshfs_mount_request(record))?;
    successful_command(&output, "sshfs")
}

/// Unmounts `mount_path`.
///
/// # Errors
///
/// Returns an error if the platform unmount command is missing or exits
/// unsuccessfully.
pub fn unmount_workspace(mount_path: &Path) -> Result<(), MountError> {
    #[cfg(target_os = "macos")]
    let requests = vec![unmount_request(mount_path)];

    #[cfg(not(target_os = "macos"))]
    let requests = vec![
        unmount_request_with("fusermount3", mount_path),
        unmount_request_with("fusermount", mount_path),
    ];

    let mut missing = Vec::new();
    for request in requests {
        match command::output(request) {
            Ok(output) => return successful_command(&output, "unmount"),
            Err(CommandError::MissingProgram(program)) => missing.push(program),
            Err(error) => return Err(MountError::CommandExec(error)),
        }
    }

    Err(MountError::CommandExec(CommandError::MissingProgram(
        missing.join(" or "),
    )))
}

/// Returns whether `path` exists on the SSH target.
///
/// # Errors
///
/// Returns an error if `ssh` cannot run or exits unexpectedly.
pub fn remote_path_exists(target: &SshTarget, path: &Path) -> Result<bool, MountError> {
    remote_test(target, "test -e", path)
}

/// Returns whether `path` is a directory on the SSH target.
///
/// # Errors
///
/// Returns an error if `ssh` cannot run or exits unexpectedly.
pub fn remote_path_is_dir(target: &SshTarget, path: &Path) -> Result<bool, MountError> {
    remote_test(target, "test -d", path)
}

/// Returns the SSH target user's home directory.
///
/// # Errors
///
/// Returns an error if `ssh` cannot run or does not return a successful home
/// directory response.
pub fn remote_home_dir(target: &SshTarget) -> Result<PathBuf, MountError> {
    let output = command::output(ssh_shell_request(target, "printf '%s\\n' \"$HOME\""))?;
    if !output.status.success() {
        return Err(MountError::Command(combined_output(&output, "ssh")));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
    ))
}

/// Lists git worktrees for `project_path` on the SSH target.
///
/// # Errors
///
/// Returns an error if `ssh` or remote `git` fails, or if git output cannot be
/// parsed.
pub fn remote_worktrees(
    target: &SshTarget,
    project_path: &Path,
) -> Result<Vec<Worktree>, MountError> {
    let output = command::output(remote_git_worktrees_request(target, project_path))?;
    Ok(git::parse_worktrees_output(&output)?)
}

/// Returns the SSH command used to list remote git worktrees.
#[must_use]
pub fn remote_git_worktrees_request(target: &SshTarget, project_path: &Path) -> CommandRequest {
    ssh_shell_request(
        target,
        &format!(
            "git -C {} worktree list --porcelain",
            shell_quote_path(project_path)
        ),
    )
}

fn remote_test(target: &SshTarget, predicate: &str, path: &Path) -> Result<bool, MountError> {
    let output = command::output(ssh_shell_request(
        target,
        &format!("{predicate} {}", shell_quote_path(path)),
    ))?;

    if output.status.success() {
        return Ok(true);
    }

    if output.status.code() == Some(1) {
        return Ok(false);
    }

    Err(MountError::Command(combined_output(&output, "ssh")))
}

fn ssh_shell_request(target: &SshTarget, remote_command: &str) -> CommandRequest {
    CommandRequest::new("ssh")
        .arg("-T")
        .arg("--")
        .arg(format!("{}@{}", target.username, target.host))
        .arg(remote_command)
}

/// Returns the structured SSHFS command for `record`.
#[must_use]
pub fn sshfs_mount_request(record: &MountRecord) -> CommandRequest {
    CommandRequest::new("sshfs")
        .args(["-o", "reconnect"])
        .args(["-o", "delay_connect"])
        .args(["-o", "idmap=user"])
        .args(["-o", "dir_cache=no"])
        .arg("--")
        .arg(format!(
            "{}@{}:{}",
            record.target.username,
            record.target.host,
            record.remote_path.display()
        ))
        .arg(record.mount_path.as_os_str())
}

/// Returns the platform default unmount command for `mount_path`.
#[must_use]
pub fn unmount_request(mount_path: &Path) -> CommandRequest {
    #[cfg(target_os = "macos")]
    {
        CommandRequest::new("umount").arg(mount_path.as_os_str())
    }

    #[cfg(not(target_os = "macos"))]
    {
        unmount_request_with("fusermount3", mount_path)
    }
}

/// Returns the deterministic local mount path for a workspace.
#[must_use]
pub fn local_mount_path(
    mounts_dir: &Path,
    machine: &str,
    project: &str,
    workspace: &MountedWorkspace,
    remote_path: &Path,
) -> PathBuf {
    let root = mounts_dir
        .join(safe_segment(machine))
        .join(safe_segment(project));

    match workspace {
        MountedWorkspace::Main => root.join("main"),
        MountedWorkspace::Branch { branch } => root.join("branches").join(format!(
            "{}-{}",
            safe_segment(branch),
            path_hash(remote_path)
        )),
        MountedWorkspace::Detached { label } => root.join("detached").join(format!(
            "{}-{}",
            safe_segment(label),
            path_hash(remote_path)
        )),
    }
}

/// Returns the path to the state file under `mounts_dir`.
#[must_use]
pub fn state_path(mounts_dir: &Path) -> PathBuf {
    mounts_dir.join(".state.json")
}

/// Loads mount state from `mounts_dir`.
///
/// # Errors
///
/// Returns an error if the state file exists but cannot be read or parsed.
pub fn load_state(mounts_dir: &Path) -> Result<MountState, MountError> {
    let path = state_path(mounts_dir);
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(MountState::default()),
        Err(error) => Err(MountError::Io(error)),
    }
}

/// Saves mount state atomically under `mounts_dir`.
///
/// # Errors
///
/// Returns an error if the state lock cannot be acquired or the file cannot be
/// written.
pub fn save_state(mounts_dir: &Path, state: &MountState) -> Result<(), MountError> {
    fs::create_dir_all(mounts_dir)?;
    let _lock = StateLock::acquire(mounts_dir)?;
    let path = state_path(mounts_dir);
    let tmp = mounts_dir.join(".state.json.tmp");
    let contents = serde_json::to_vec_pretty(state)?;

    {
        let mut file = File::create(&tmp)?;
        file.write_all(&contents)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }

    fs::rename(tmp, path)?;
    Ok(())
}

/// Adds or replaces `record` in `state`.
pub fn upsert_record(state: &mut MountState, record: MountRecord) {
    state.mounts.retain(|candidate| {
        !(candidate.machine == record.machine
            && candidate.project == record.project
            && candidate.workspace == record.workspace)
    });
    state.mounts.push(record);
}

/// Removes records whose mount path equals `mount_path`.
pub fn remove_record(state: &mut MountState, mount_path: &Path) {
    state
        .mounts
        .retain(|record| record.mount_path != mount_path);
}

/// Lists known mounts and annotates them with OS mount-table status.
///
/// # Errors
///
/// Returns an error if state or the mount table cannot be read.
pub fn list_known_mounts(
    mounts_dir: &Path,
    machine: &str,
) -> Result<Vec<WorkspaceMount>, MountError> {
    let state = load_state(mounts_dir)?;
    let mounted_paths = mounted_paths()?;
    Ok(status_records(state, &mounted_paths, machine))
}

/// Removes stale records from state.
///
/// A stale record is one whose mountpoint path is missing or not listed by the
/// OS mount table.
///
/// # Errors
///
/// Returns an error if state or the mount table cannot be read or state cannot
/// be saved.
pub fn prune_stale(mounts_dir: &Path, machine: &str) -> Result<usize, MountError> {
    let mut state = load_state(mounts_dir)?;
    let before = state.mounts.len();
    let mounted_paths = mounted_paths()?;
    state.mounts.retain(|record| {
        record.machine != machine
            || (record.mount_path.exists() && mounted_paths.contains(&record.mount_path))
    });
    let removed = before - state.mounts.len();
    save_state(mounts_dir, &state)?;
    Ok(removed)
}

fn status_records(
    state: MountState,
    mounted_paths: &HashSet<PathBuf>,
    machine: &str,
) -> Vec<WorkspaceMount> {
    state
        .mounts
        .into_iter()
        .filter(|record| record.machine == machine)
        .map(|record| {
            let status = if !record.mount_path.exists() {
                MountStatus::Missing
            } else if mounted_paths.contains(&record.mount_path) {
                MountStatus::Mounted
            } else {
                MountStatus::Recorded
            };

            WorkspaceMount { record, status }
        })
        .collect()
}

fn mounted_paths() -> Result<HashSet<PathBuf>, MountError> {
    let output = command::output(CommandRequest::new("mount"))?;
    if !output.status.success() {
        return Err(MountError::Command(combined_output(&output, "mount")));
    }

    Ok(parse_mount_table(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_mount_table(input: &str) -> HashSet<PathBuf> {
    input
        .lines()
        .filter_map(parse_mount_line)
        .collect::<HashSet<_>>()
}

fn parse_mount_line(line: &str) -> Option<PathBuf> {
    if let Some((_, rest)) = line.split_once(" on ")
        && let Some((path, _)) = rest.split_once(" (")
    {
        return Some(PathBuf::from(path));
    }

    if let Some((_, rest)) = line.split_once(' ')
        && let Some((path, _)) = rest.split_once(" type ")
    {
        return Some(PathBuf::from(path));
    }

    None
}

fn unmount_request_with(program: &str, mount_path: &Path) -> CommandRequest {
    CommandRequest::new(program)
        .arg("-u")
        .arg(mount_path.as_os_str())
}

fn successful_command(output: &CommandOutput, name: &str) -> Result<(), MountError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(MountError::Command(combined_output(output, name)))
    }
}

fn combined_output(output: &CommandOutput, name: &str) -> String {
    let mut message = format!("{name} exited with status {}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        message.push_str("\nstdout:\n");
        message.push_str(stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        message.push_str("\nstderr:\n");
        message.push_str(stderr.trim_end());
    }
    message
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn safe_segment(input: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_dash = false;

    for ch in input.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };

        if next == '-' {
            if last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }

        sanitized.push(next);
    }

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "workspace".to_owned()
    } else {
        sanitized.to_owned()
    }
}

fn path_hash(path: &Path) -> String {
    let mut hasher = FnvHasher::default();
    path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
        .chars()
        .take(8)
        .collect()
}

#[derive(Default)]
struct FnvHasher(u64);

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.0 == 0 {
            self.0 = 0xcbf2_9ce4_8422_2325;
        }
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

struct StateLock {
    path: PathBuf,
}

impl StateLock {
    fn acquire(mounts_dir: &Path) -> Result<Self, MountError> {
        let path = mounts_dir.join(".state.lock");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                file.sync_all()?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(MountError::StateLocked(path))
            }
            Err(error) => Err(MountError::Io(error)),
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_path_for_main_is_deterministic() {
        let path = local_mount_path(
            Path::new("/mounts"),
            "worker",
            "piquel-cli",
            &MountedWorkspace::Main,
            Path::new("/home/me/piquel-cli"),
        );

        assert_eq!(path, PathBuf::from("/mounts/worker/piquel-cli/main"));
    }

    #[test]
    fn branch_mount_path_sanitizes_and_hashes_remote_path() {
        let left = local_mount_path(
            Path::new("/mounts"),
            "worker",
            "piquel-cli",
            &MountedWorkspace::Branch {
                branch: "feature/login".to_owned(),
            },
            Path::new("/home/me/repo one"),
        );
        let right = local_mount_path(
            Path::new("/mounts"),
            "worker",
            "piquel-cli",
            &MountedWorkspace::Branch {
                branch: "feature/login".to_owned(),
            },
            Path::new("/home/me/repo two"),
        );

        assert_ne!(left, right);
        assert!(left.to_string_lossy().contains("/branches/feature-login-"));
    }

    #[test]
    fn sshfs_request_uses_structured_arguments() {
        let record = MountRecord {
            machine: "worker".to_owned(),
            project: "alpha".to_owned(),
            workspace: MountedWorkspace::Main,
            target: SshTarget {
                username: "ro'nan".to_owned(),
                host: "host".to_owned(),
            },
            remote_path: PathBuf::from("/remote/path with spaces"),
            mount_path: PathBuf::from("/local/mount with spaces"),
        };

        assert_eq!(
            sshfs_mount_request(&record),
            CommandRequest::new("sshfs")
                .args(["-o", "reconnect"])
                .args(["-o", "delay_connect"])
                .args(["-o", "idmap=user"])
                .args(["-o", "dir_cache=no"])
                .arg("--")
                .arg("ro'nan@host:/remote/path with spaces")
                .arg("/local/mount with spaces")
        );
    }

    #[test]
    fn parses_macos_and_linux_mount_tables() {
        let paths = parse_mount_table(
            "user@host:/repo on /Users/me/.piquel/mounts/worker/repo/main (macfuse, nodev)\n/dev/sda1 on / type ext4 (rw)\nfuse.sshfs /home/me/.piquel/mounts/worker/repo/main type fuse.sshfs (rw)\n",
        );

        assert!(paths.contains(Path::new("/Users/me/.piquel/mounts/worker/repo/main")));
        assert!(paths.contains(Path::new("/home/me/.piquel/mounts/worker/repo/main")));
    }

    #[test]
    fn state_round_trips_and_updates_records() {
        let dir = std::env::temp_dir().join(format!(
            "piquel-mount-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be valid")
                .as_nanos()
        ));
        let mut state = MountState::default();
        let record = MountRecord {
            machine: "worker".to_owned(),
            project: "alpha".to_owned(),
            workspace: MountedWorkspace::Main,
            target: SshTarget {
                username: "me".to_owned(),
                host: "host".to_owned(),
            },
            remote_path: PathBuf::from("/remote"),
            mount_path: dir.join("worker/alpha/main"),
        };

        upsert_record(&mut state, record.clone());
        upsert_record(&mut state, record);
        save_state(&dir, &state).expect("state should save");
        let loaded = load_state(&dir).expect("state should load");

        assert_eq!(loaded.mounts.len(), 1);
        fs::remove_dir_all(dir).expect("test dir should be removed");
    }
}
