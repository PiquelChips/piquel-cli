use std::error::Error;

use crate::{config, git, tmux};

pub fn list_projects() -> Result<(), Box<dyn Error>> {
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

    Ok(())
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
            "Project \"{}\" path {:?} does not exist; configured repository is {}",
            project.name, project.path, project.repository
        )
        .into());
    }

    if !project.path.is_dir() {
        return Err(format!(
            "Project \"{}\" path {:?} is not a directory; configured repository is {}",
            project.name, project.path, project.repository
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
