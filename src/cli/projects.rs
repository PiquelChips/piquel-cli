use std::error::Error;

use crate::{config, fzf, git, tmux};

pub fn list_projects() {
    let config = config::config();
    let mut projects = config
        .projects
        .iter()
        .filter_map(|project| project.resolved_name().ok())
        .collect::<Vec<_>>();

    projects.sort();
    projects.dedup();

    for project in projects {
        println!("{project}");
    }
}

pub fn load_project(
    project_name: &str,
    session_override: Option<&str>,
    worktree: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

    let config = config::config();
    let project = config
        .project(project_name)
        .ok_or_else(|| format!("Project \"{project_name}\" is not configured"))?;

    let template = config
        .project_session_template(&project, session_override)
        .ok_or_else(|| {
            let template_name = session_override.unwrap_or("<project default>");
            format!("Session template \"{template_name}\" is not configured")
        })?;

    if !project.path.exists() {
        return Err(format!(
            "Project \"{}\" path {} does not exist; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        )
        .into());
    }

    if !project.path.is_dir() {
        return Err(format!(
            "Project \"{}\" path {} is not a directory; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        )
        .into());
    }

    let (root, tmux_name) = match worktree {
        Some(branch) => {
            let worktree = git::find_worktree(&project.path, branch)?;
            (worktree.path, format!("{}--{branch}", project.name))
        }
        None => (project.path.clone(), project.name.clone()),
    };

    tmux::open_session(&tmux_name, &root, template)?;
    Ok(())
}

pub fn open_project_interactive(project_name: &str) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

    let config = config::config();
    let project = config
        .project(project_name)
        .ok_or_else(|| format!("Project \"{project_name}\" is not configured"))?;

    let template = config
        .project_session_template(&project, None)
        .ok_or_else(|| "Project default session template is not configured".to_owned())?;

    if !project.path.exists() {
        return Err(format!(
            "Project \"{}\" path {} does not exist; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        )
        .into());
    }

    if !project.path.is_dir() {
        return Err(format!(
            "Project \"{}\" path {} is not a directory; configured repository is {}",
            project.name,
            project.path.display(),
            project.repository
        )
        .into());
    }

    let worktrees = git::list_worktrees(&project.path)?;
    if !git::has_additional_worktrees(&project.path, &worktrees) {
        tmux::open_session(&project.name, &project.path, template)?;
        return Ok(());
    }

    let branch_worktrees = git::branch_worktree_choices(&worktrees);
    let branch_names = branch_worktrees
        .iter()
        .map(|(branch, _)| branch.clone())
        .collect::<Vec<_>>();

    let Some(branch) = fzf::select(branch_names, "worktree> ")? else {
        return Ok(());
    };

    let root = branch_worktrees
        .into_iter()
        .find_map(|(candidate, path)| (candidate == branch).then_some(path))
        .ok_or_else(|| format!("Selected unknown worktree branch \"{branch}\""))?;
    let tmux_name = format!("{}--{branch}", project.name);

    tmux::open_session(&tmux_name, &root, template)?;
    Ok(())
}
