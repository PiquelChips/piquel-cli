use std::fs;

use anyhow::{Context, Result, anyhow, bail};

use crate::{Config, ResolvedProject, SessionConfig, cli::State, fzf, git, tmux};

impl State {
    /// Lists configured projects.
    ///
    /// # Errors
    ///
    /// This currently does not fail, but returns `Result` to match CLI command
    /// handler dispatch.
    pub fn list_projects(&self) -> Result<()> {
        let mut projects = self
            .config
            .projects
            .iter()
            .filter_map(|project| project.resolved_name().ok())
            .collect::<Vec<_>>();

        projects.sort();
        projects.dedup();

        for project in projects {
            println!("{project}");
        }

        Ok(())
    }

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

        validate_project_path(&project)?;

        match worktree {
            Some(branch) => open_project_branch(&self.config, &project, template, branch)?,
            None => tmux::open_session(&project.name, &project.path, template)?,
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

        validate_project_path(&project)?;

        let branches = git::list_local_branches(&project.path)?;
        if branches.is_empty() {
            tmux::open_session(&project.name, &project.path, template)?;
            return Ok(());
        }

        let Some(branch) = fzf::select(branches, "branch> ")? else {
            return Ok(());
        };

        open_project_branch(&self.config, &project, template, &branch)?;
        Ok(())
    }
}

fn validate_project_path(project: &ResolvedProject) -> Result<()> {
    if !project.path.exists() {
        bail!(
            "Project \"{}\" path {} does not exist; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        );
    }

    if !project.path.is_dir() {
        bail!(
            "Project \"{}\" path {} is not a directory; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        );
    }

    Ok(())
}

fn open_project_branch(
    config: &Config,
    project: &ResolvedProject,
    template: &SessionConfig,
    branch: &str,
) -> Result<()> {
    let branches = git::list_local_branches(&project.path)?;
    if !branches.iter().any(|candidate| candidate == branch) {
        bail!(
            "Branch \"{}\" is not a local branch for project \"{}\"",
            branch,
            project.name
        );
    }

    let worktrees = git::list_worktrees(&project.path)?;
    let root = if let Some(worktree) = git::worktree_for_branch(&worktrees, branch) {
        worktree.path
    } else {
        let worktree_path = git::managed_worktree_path_for_branch(
            &config.worktrees_dir,
            &project.name,
            branch,
            &worktrees,
        )?;
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create managed worktree directory {}",
                    parent.display()
                )
            })?;
        }
        git::create_worktree(&project.path, &worktree_path, branch)?;
        worktree_path
    };

    let tmux_name = format!("{}--{branch}", project.name);
    tmux::open_session(&tmux_name, &root, template)?;
    Ok(())
}
