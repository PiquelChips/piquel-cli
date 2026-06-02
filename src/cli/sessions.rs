use std::{
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
};

use crate::{cli::projects, config, fzf, tmux};

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickTarget {
    TmuxSession(String),
    Project(String),
}

pub fn pick() -> Result<(), Box<dyn Error>> {
    match pick_target()? {
        Some(PickTarget::TmuxSession(name)) => {
            tmux::err_in_tmux()?;
            tmux::attach(&name)?;
        }
        Some(PickTarget::Project(name)) => projects::open_project_interactive(&name)?,
        None => {}
    }

    Ok(())
}

pub fn session(
    path: Option<PathBuf>,
    session_override: Option<&str>,
    name_override: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    tmux::err_in_tmux()?;

    let config = config::config();
    let template_name = session_override.unwrap_or(&config.default_session);
    let template = config
        .session_template(template_name)
        .ok_or_else(|| format!("Session template \"{template_name}\" is not configured"))?;

    let root = match path {
        Some(path) => expand_home(&path),
        None => std::env::current_dir()?,
    };

    if !root.exists() {
        return Err(format!("Session path {root:?} does not exist").into());
    }

    if !root.is_dir() {
        return Err(format!("Session path {root:?} is not a directory").into());
    }

    let tmux_name = match name_override {
        Some(name) => name.to_owned(),
        None => root
            .file_name()
            .ok_or_else(|| format!("Could not derive session name from path {root:?}"))?
            .to_string_lossy()
            .into_owned(),
    };

    tmux::open_session(&tmux_name, &root, template)?;
    Ok(())
}

fn expand_home(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~")
        && let Some(home) = std::env::home_dir()
    {
        return home.join(stripped);
    }
    path.to_path_buf()
}

fn pick_target() -> Result<Option<PickTarget>, Box<dyn Error>> {
    let config = config::config();
    let project_names = config
        .projects
        .iter()
        .filter_map(|project| project.resolved_name().ok());
    let tmux_sessions = tmux::list_tmux_sessions()?;
    let (items, mut targets) = build_pick_targets(tmux_sessions, project_names);

    let Some(selection) = fzf::select(items, "piquel> ")? else {
        return Ok(None);
    };

    targets
        .remove(&selection)
        .map(Some)
        .ok_or_else(|| format!("Selected unknown picker item \"{selection}\"").into())
}

fn build_pick_targets<T, P>(
    tmux_sessions: T,
    project_names: P,
) -> (Vec<String>, HashMap<String, PickTarget>)
where
    T: IntoIterator<Item = String>,
    P: IntoIterator<Item = String>,
{
    let mut targets = HashMap::new();

    for project in project_names {
        targets.insert(project.clone(), PickTarget::Project(project));
    }

    for session in tmux_sessions {
        targets.insert(session.clone(), PickTarget::TmuxSession(session));
    }

    let mut items = targets.keys().cloned().collect::<Vec<_>>();
    items.sort();
    (items, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_collision_prefers_tmux_session() {
        let (_, targets) = build_pick_targets(
            vec!["shared".to_owned()],
            vec!["shared".to_owned(), "project".to_owned()],
        );

        assert_eq!(
            targets.get("shared"),
            Some(&PickTarget::TmuxSession("shared".to_owned()))
        );
    }

    #[test]
    fn picker_items_are_sorted_and_deduplicated() {
        let (items, _) = build_pick_targets(
            vec!["zeta".to_owned(), "alpha".to_owned()],
            vec!["zeta".to_owned(), "beta".to_owned()],
        );

        assert_eq!(items, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn project_only_name_maps_to_project() {
        let (_, targets) = build_pick_targets(Vec::<String>::new(), vec!["project".to_owned()]);

        assert_eq!(
            targets.get("project"),
            Some(&PickTarget::Project("project".to_owned()))
        );
    }

    #[test]
    fn tmux_only_name_maps_to_tmux_session() {
        let (_, targets) = build_pick_targets(vec!["session".to_owned()], Vec::<String>::new());

        assert_eq!(
            targets.get("session"),
            Some(&PickTarget::TmuxSession("session".to_owned()))
        );
    }
}
