use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

/// A git worktree discovered from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Filesystem path to the worktree.
    pub path: PathBuf,
    /// Local branch checked out by the worktree, if any.
    pub branch: Option<String>,
}

/// Errors produced while discovering git worktrees.
#[derive(Debug, Error)]
pub enum GitError {
    /// The git process could not be spawned or observed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    /// git exited unsuccessfully.
    #[error("{0}")]
    Command(String),
    /// The configured project path does not exist.
    #[error("Project path {} does not exist", .0.display())]
    MissingProjectPath(PathBuf),
    /// No worktree exists for the requested branch.
    #[error("No local git worktree for branch \"{branch}\" exists under {}", project_path.display())]
    MissingWorktree {
        /// Branch that was requested.
        branch: String,
        /// Project path that was searched.
        project_path: PathBuf,
    },
    /// The branch cannot be converted into a managed worktree path segment.
    #[error("Branch \"{0}\" cannot be used as a managed worktree path segment")]
    InvalidManagedWorktreeBranch(String),
    /// The managed worktree path exists but belongs to another checkout.
    #[error("Managed worktree path {} already exists but is not registered for branch \"{branch}\"", path.display())]
    ManagedWorktreePathConflict {
        /// Existing path that would be reused for the managed worktree.
        path: PathBuf,
        /// Branch that was requested.
        branch: String,
    },
    /// git worktree output could not be parsed.
    #[error("{0}")]
    Parse(String),
}

/// Lists local branch names for `project_path`, sorted by git.
///
/// # Errors
///
/// Returns an error if `project_path` does not exist or git fails.
pub fn list_local_branches(project_path: &Path) -> Result<Vec<String>, GitError> {
    if !project_path.exists() {
        return Err(GitError::MissingProjectPath(project_path.to_owned()));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .output()
        .map_err(GitError::Io)?;

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };

    if !output.status.success() {
        return Err(GitError::Command(combined));
    }

    Ok(parse_local_branches(&combined))
}

/// Creates a git worktree for `branch` at `worktree_path`.
///
/// # Errors
///
/// Returns an error if git fails.
pub fn create_worktree(
    project_path: &Path,
    worktree_path: &Path,
    branch: &str,
) -> Result<(), GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["worktree", "add"])
        .arg(worktree_path)
        .arg(branch)
        .output()
        .map_err(GitError::Io)?;

    if output.status.success() {
        return Ok(());
    }

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };
    Err(GitError::Command(combined))
}

/// Lists all git worktrees for `project_path`.
///
/// # Errors
///
/// Returns an error if `project_path` does not exist, git fails, or the
/// porcelain output cannot be parsed.
pub fn list_worktrees(project_path: &Path) -> Result<Vec<Worktree>, GitError> {
    if !project_path.exists() {
        return Err(GitError::MissingProjectPath(project_path.to_owned()));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(GitError::Io)?;

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&output.stderr));
        s
    };

    if !output.status.success() {
        return Err(GitError::Command(combined));
    }

    parse_worktrees(&combined)
}

/// Finds the worktree for `branch` under `project_path`.
///
/// # Errors
///
/// Returns an error if worktree listing fails or no worktree matches `branch`.
pub fn find_worktree(project_path: &Path, branch: &str) -> Result<Worktree, GitError> {
    find_worktree_in(list_worktrees(project_path)?, project_path, branch)
}

/// Finds the worktree for `branch` in an already-discovered worktree list.
#[must_use]
pub fn worktree_for_branch(worktrees: &[Worktree], branch: &str) -> Option<Worktree> {
    worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
        .cloned()
}

fn find_worktree_in(
    worktrees: Vec<Worktree>,
    project_path: &Path,
    branch: &str,
) -> Result<Worktree, GitError> {
    worktrees
        .into_iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
        .ok_or_else(|| GitError::MissingWorktree {
            branch: branch.to_owned(),
            project_path: project_path.to_owned(),
        })
}

fn parse_worktrees(input: &str) -> Result<Vec<Worktree>, GitError> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in input.lines() {
        if line.is_empty() {
            push_worktree(&mut worktrees, current_path.take(), current_branch.take())?;
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            push_worktree(&mut worktrees, current_path.take(), current_branch.take())?;
            current_path = Some(PathBuf::from(path));
            continue;
        }

        if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.to_owned());
        }
    }

    push_worktree(&mut worktrees, current_path.take(), current_branch.take())?;
    Ok(worktrees)
}

fn parse_local_branches(input: &str) -> Vec<String> {
    let mut branches = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    branches.sort();
    branches
}

fn push_worktree(
    worktrees: &mut Vec<Worktree>,
    path: Option<PathBuf>,
    branch: Option<String>,
) -> Result<(), GitError> {
    if let Some(path) = path {
        worktrees.push(Worktree { path, branch });
    } else if branch.is_some() {
        return Err(GitError::Parse(
            "Found git worktree branch before worktree path".to_owned(),
        ));
    }

    Ok(())
}

/// Returns whether `worktrees` includes a branch worktree beyond `project_path`.
#[must_use]
pub fn has_additional_worktrees(project_path: &Path, worktrees: &[Worktree]) -> bool {
    let branch_worktrees = worktrees
        .iter()
        .filter(|worktree| worktree.branch.is_some())
        .collect::<Vec<_>>();

    branch_worktrees.len() > 1
        || branch_worktrees
            .iter()
            .any(|worktree| !same_path(&worktree.path, project_path))
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Returns sorted `(branch, path)` choices for branch worktrees.
#[must_use]
pub fn branch_worktree_choices(worktrees: &[Worktree]) -> Vec<(String, PathBuf)> {
    let mut choices = worktrees
        .iter()
        .filter_map(|worktree| Some((worktree.branch.clone()?, worktree.path.clone())))
        .collect::<Vec<_>>();
    choices.sort_by(|(left, _), (right, _)| left.cmp(right));
    choices
}

/// Returns the managed worktree path for `project_name` and `branch`.
///
/// # Errors
///
/// Returns an error when `branch` cannot produce a non-empty path segment.
pub fn managed_worktree_path(
    worktrees_dir: &Path,
    project_name: &str,
    branch: &str,
) -> Result<PathBuf, GitError> {
    let branch_segment = sanitized_branch_segment(branch);
    if branch_segment.is_empty() {
        return Err(GitError::InvalidManagedWorktreeBranch(branch.to_owned()));
    }

    Ok(worktrees_dir.join(project_name).join(branch_segment))
}

/// Returns a managed worktree path and rejects path collisions.
///
/// # Errors
///
/// Returns an error if the branch path cannot be generated or if an existing
/// path is not already registered as `branch`'s worktree.
pub fn managed_worktree_path_for_branch(
    worktrees_dir: &Path,
    project_name: &str,
    branch: &str,
    worktrees: &[Worktree],
) -> Result<PathBuf, GitError> {
    let path = managed_worktree_path(worktrees_dir, project_name, branch)?;

    if path.exists()
        && !worktrees.iter().any(|worktree| {
            same_path(&worktree.path, &path) && worktree.branch.as_deref() == Some(branch)
        })
    {
        return Err(GitError::ManagedWorktreePathConflict {
            path,
            branch: branch.to_owned(),
        });
    }

    Ok(path)
}

fn sanitized_branch_segment(input: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_underscore = false;

    for ch in input.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ch
        } else {
            '_'
        };

        if next == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
        } else {
            last_was_underscore = false;
        }

        sanitized.push(next);
    }

    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_worktrees_and_branch_names_with_slashes() {
        let output = "\
worktree /home/me/Projects/repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/me/Projects/repo-feature
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature/foo

";

        let worktrees = parse_worktrees(output).expect("worktree output should parse");

        assert_eq!(
            worktrees,
            vec![
                Worktree {
                    path: PathBuf::from("/home/me/Projects/repo"),
                    branch: Some("main".to_owned())
                },
                Worktree {
                    path: PathBuf::from("/home/me/Projects/repo-feature"),
                    branch: Some("feature/foo".to_owned())
                }
            ]
        );
    }

    #[test]
    fn detached_worktrees_have_no_branch() {
        let output = "\
worktree /home/me/Projects/repo-detached
HEAD 3333333333333333333333333333333333333333
detached

";

        let worktrees = parse_worktrees(output).expect("worktree output should parse");

        assert_eq!(worktrees[0].branch, None);
    }

    #[test]
    fn exact_branch_lookup_ignores_detached_head() {
        let output = "\
worktree /home/me/Projects/repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/me/Projects/repo-detached
HEAD 3333333333333333333333333333333333333333
detached

";

        let worktrees = parse_worktrees(output).expect("worktree output should parse");
        let found = find_worktree_in(worktrees, Path::new("/home/me/Projects/repo"), "main")
            .expect("main worktree should exist");

        assert_eq!(found.path, PathBuf::from("/home/me/Projects/repo"));
    }

    #[test]
    fn missing_branch_returns_an_error() {
        let output = "\
worktree /home/me/Projects/repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/me/Projects/repo-detached
HEAD 3333333333333333333333333333333333333333
detached

";

        let err = find_worktree_in(
            parse_worktrees(output).expect("worktree output should parse"),
            Path::new("/home/me/Projects/repo"),
            "feature/foo",
        )
        .expect_err("missing branch should return an error");

        assert!(err.to_string().contains("feature/foo"));
    }

    #[test]
    fn parses_and_sorts_local_branches() {
        let branches = parse_local_branches(
            "\
zeta
feature/foo

main
",
        );

        assert_eq!(branches, vec!["feature/foo", "main", "zeta"]);
    }

    #[test]
    fn parses_empty_local_branch_list() {
        assert!(parse_local_branches("\n").is_empty());
    }

    #[test]
    fn worktree_for_branch_matches_exact_branch() {
        let worktrees = vec![
            worktree("/repo", Some("main")),
            worktree("/repo-feature", Some("feature/foo")),
            worktree("/repo-featured", Some("feature")),
        ];

        let found =
            worktree_for_branch(&worktrees, "feature/foo").expect("branch worktree should exist");

        assert_eq!(found.path, PathBuf::from("/repo-feature"));
    }

    #[test]
    fn generates_managed_worktree_path() {
        let path = managed_worktree_path(Path::new("/worktrees"), "alpha", "feature/foo")
            .expect("managed path should be generated");

        assert_eq!(path, PathBuf::from("/worktrees/alpha/feature_foo"));
    }

    #[test]
    fn rejects_empty_managed_worktree_path_segment() {
        let err = managed_worktree_path(Path::new("/worktrees"), "alpha", "   ")
            .expect_err("empty branch should be rejected");

        assert!(matches!(err, GitError::InvalidManagedWorktreeBranch(_)));
    }

    #[test]
    fn rejects_existing_unregistered_managed_worktree_path() {
        let root = std::env::temp_dir().join(format!(
            "piquel-git-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be valid")
                .as_nanos()
        ));
        let existing = root.join("alpha/feature_foo");
        std::fs::create_dir_all(&existing).expect("test directory should be created");

        let err = managed_worktree_path_for_branch(&root, "alpha", "feature/foo", &[])
            .expect_err("existing unregistered path should be rejected");

        assert!(matches!(err, GitError::ManagedWorktreePathConflict { .. }));

        std::fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn worktree_picker_includes_all_branch_worktrees() {
        let worktrees = vec![
            worktree("/repo", Some("main")),
            worktree("/repo-feature", Some("feature/foo")),
        ];

        let choices = branch_worktree_choices(&worktrees)
            .into_iter()
            .map(|(branch, _)| branch)
            .collect::<Vec<_>>();

        assert_eq!(choices, vec!["feature/foo", "main"]);
    }

    #[test]
    fn detached_worktrees_are_excluded_from_picker_choices() {
        let worktrees = vec![
            worktree("/repo", Some("main")),
            worktree("/repo-detached", None),
        ];

        let choices = branch_worktree_choices(&worktrees)
            .into_iter()
            .map(|(branch, _)| branch)
            .collect::<Vec<_>>();

        assert_eq!(choices, vec!["main"]);
    }

    #[test]
    fn no_additional_branch_worktrees_falls_back_to_project_root() {
        let project_path = Path::new("/repo");
        let worktrees = vec![
            worktree("/repo", Some("main")),
            worktree("/repo-detached", None),
        ];

        assert!(!has_additional_worktrees(project_path, &worktrees));
    }

    #[test]
    fn additional_branch_worktrees_trigger_worktree_picker() {
        let project_path = Path::new("/repo");

        assert!(has_additional_worktrees(
            project_path,
            &[
                worktree("/repo", Some("main")),
                worktree("/repo-feature", Some("feature/foo"))
            ]
        ));
        assert!(has_additional_worktrees(
            project_path,
            &[worktree("/repo-feature", Some("feature/foo"))]
        ));
    }

    fn worktree(path: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: PathBuf::from(path),
            branch: branch.map(str::to_owned),
        }
    }
}
