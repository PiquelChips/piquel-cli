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
    /// git worktree output could not be parsed.
    #[error("{0}")]
    Parse(String),
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
