use std::io::{self, Write};

use anyhow::{Context, Result, anyhow, bail};

use crate::{ResolvedProject, SessionConfig, cli::State, git, tmux};

impl State {
    /// Loads a configured project, optionally opening a branch worktree.
    ///
    /// # Errors
    ///
    /// Returns an error if the command is run inside tmux, the project or
    /// session template is not configured, the project path is invalid, git
    /// worktree setup fails, or tmux cannot open the session.
    pub fn load_project(
        &self,
        project_name: &str,
        session_override: Option<&str>,
        worktree: Option<&str>,
    ) -> Result<()> {
        tmux::err_in_tmux()?;

        let project = self
            .config
            .project(project_name)
            .ok_or_else(|| anyhow!("Project \"{project_name}\" is not configured"))?;

        let template = self
            .config
            .project_session_template(&project, session_override)
            .ok_or_else(|| {
                let template_name = session_override.unwrap_or("<project default>");
                anyhow!("Session template \"{template_name}\" is not configured")
            })?;

        self.ensure_project_path(&project)?;

        match worktree {
            Some(branch) => self.open_project_branch(&project, template, branch)?,
            None => tmux::open_session(self.backend(), &project.name, &project.path, template)?,
        }

        Ok(())
    }

    /// Opens a configured project, prompting for a local branch when available.
    ///
    /// # Errors
    ///
    /// Returns an error if the command is run inside tmux, the project or
    /// session template is not configured, the project path is invalid, branch
    /// selection fails, git worktree setup fails, or tmux cannot open the
    /// session.
    pub fn open_project_interactive(
        &self,
        project_name: &str,
        session_override: Option<&str>,
    ) -> Result<()> {
        tmux::err_in_tmux()?;

        let project = self
            .config
            .project(project_name)
            .ok_or_else(|| anyhow!("Project \"{project_name}\" is not configured"))?;

        let template = self
            .config
            .project_session_template(&project, session_override)
            .ok_or_else(|| {
                let template_name = session_override.unwrap_or("<project default>");
                anyhow!("Session template \"{template_name}\" is not configured")
            })?;

        self.ensure_project_path(&project)?;

        let branches = self.list_local_branches(&project.path)?;
        if branches.is_empty() {
            tmux::open_session(self.backend(), &project.name, &project.path, template)?;
            return Ok(());
        }

        let Some(branch) = Self::select_fzf(branches, "branch> ")? else {
            return Ok(());
        };

        self.open_project_branch(&project, template, &branch)?;
        Ok(())
    }

    fn open_project_branch(
        &self,
        project: &ResolvedProject,
        template: &SessionConfig,
        branch: &str,
    ) -> Result<()> {
        let branches = self.list_local_branches(&project.path)?;
        if !branches.iter().any(|candidate| candidate == branch) {
            bail!(
                "Branch \"{}\" is not a local branch for project \"{}\"",
                branch,
                project.name
            );
        }

        let worktrees = self.list_worktrees(&project.path)?;
        let root = if let Some(worktree) = git::worktree_for_branch(&worktrees, branch) {
            worktree.path
        } else {
            let worktree_path = git::managed_worktree_path_for_branch(
                self.backend(),
                &self.config.worktrees_dir,
                &project.name,
                branch,
                &worktrees,
            )?;
            if let Some(parent) = worktree_path.parent() {
                self.backend().create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create managed worktree directory {}",
                        parent.display()
                    )
                })?;
            }
            self.create_worktree(&project.path, &worktree_path, branch)?;
            worktree_path
        };

        let tmux_name = format!("{}--{branch}", project.name);
        tmux::open_session(self.backend(), &tmux_name, &root, template)?;
        Ok(())
    }

    fn list_local_branches(&self, project_path: &std::path::Path) -> Result<Vec<String>> {
        let output = self.backend().output(git::list_local_branches_request(
            self.backend(),
            project_path,
        )?)?;
        Ok(git::parse_local_branches_output(&output)?)
    }

    fn list_worktrees(&self, project_path: &std::path::Path) -> Result<Vec<git::Worktree>> {
        let output = self
            .backend()
            .output(git::list_worktrees_request(self.backend(), project_path)?)?;
        Ok(git::parse_worktrees_output(&output)?)
    }

    fn create_worktree(
        &self,
        project_path: &std::path::Path,
        worktree_path: &std::path::Path,
        branch: &str,
    ) -> Result<()> {
        let output = self.backend().output(git::create_worktree_request(
            project_path,
            worktree_path,
            branch,
        ))?;
        Ok(git::parse_create_worktree_output(&output)?)
    }

    fn ensure_project_path(&self, project: &ResolvedProject) -> Result<()> {
        if self.backend().path_exists(&project.path)? {
            if !self.backend().path_is_dir(&project.path)? {
                bail!(
                    "Project \"{}\" path {} is not a directory",
                    project.name,
                    project.path.display(),
                );
            }
            return Ok(());
        }

        if !prompt_clone_project(project)? {
            bail!(
                "Project \"{}\" path {} does not exist; clone cancelled",
                project.name,
                project.path.display()
            );
        }

        clone_project(self.backend(), project)
    }
}

fn prompt_clone_project(project: &ResolvedProject) -> Result<bool> {
    eprint!(
        "Project \"{}\" path {} does not exist. Clone {} there? [y/N] ",
        project.name,
        project.path.display(),
        project.repository
    );
    io::stderr().flush().context("Failed to flush stderr")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("Failed to read clone confirmation")?;

    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn clone_project(backend: &dyn crate::backend::Backend, project: &ResolvedProject) -> Result<()> {
    if let Some(parent) = project.path.parent() {
        backend.create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create project parent directory {}",
                parent.display()
            )
        })?;
    }

    let status = backend.status(git::clone_repository_request(
        &project.repository,
        &project.path,
    ))?;
    if !status.success() {
        bail!(
            "Failed to clone repository {} into {}",
            project.repository,
            project.path.display()
        );
    }

    Ok(())
}
