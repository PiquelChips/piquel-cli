pub mod cli;
pub mod config;
pub mod git;
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::config::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    windows: Vec<WindowConfig>,
}

impl SessionConfig {
    fn validate(&self, name: &str) -> Result<(), config::ConfigError> {
        if name.trim().is_empty() || name.contains(':') {
            return Err(ConfigError::Validation(format!(
                "\"{name}\" is not a valid session template name"
            )));
        }

        if self.windows.is_empty() {
            return Err(ConfigError::Validation(format!(
                "Session template \"{name}\" must have at least one window"
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    repository: String,
    name: Option<String>,
    path: Option<PathBuf>,
    default_session: Option<ProjectSessionConfig>,
}

impl ProjectConfig {
    pub fn resolved_name(&self) -> Result<String, ConfigError> {
        let name = match &self.name {
            Some(name) => name.clone(),
            None => repository_basename(&self.repository)?,
        };

        validate_project_name(&name)?;
        Ok(name)
    }

    pub fn resolved_path(&self, projects_dir: &Path) -> Result<PathBuf, ConfigError> {
        match &self.path {
            Some(path) => Ok(expand_home(path)),
            None => Ok(projects_dir.join(self.resolved_name()?)),
        }
    }

    pub fn resolved_default_session(&self, config: &Config) -> ProjectSessionConfig {
        self.default_session
            .clone()
            .unwrap_or_else(|| ProjectSessionConfig::Template(config.default_session.clone()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ProjectSessionConfig {
    Template(String),
    Inline(SessionConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_projects_dir")]
    projects_dir: PathBuf,
    #[serde(default = "default_default_session")]
    default_session: String,
    #[serde(default)]
    sessions: HashMap<String, SessionConfig>,
    #[serde(default)]
    projects: Vec<ProjectConfig>,
}

impl Config {
    pub fn validate_and_normalize(&mut self) -> Result<(), ConfigError> {
        self.projects_dir = expand_home(&self.projects_dir);

        for (name, session) in &self.sessions {
            session.validate(name)?;
        }

        if !self.sessions.contains_key(&self.default_session) {
            return Err(ConfigError::Validation(format!(
                "Default session template \"{}\" does not exist",
                self.default_session
            )));
        }

        let mut project_names = HashSet::new();
        let global_default_session = self.default_session.clone();

        for project in &mut self.projects {
            let name = project.resolved_name()?;
            if !project_names.insert(name.clone()) {
                return Err(ConfigError::Validation(format!(
                    "Duplicate project name \"{name}\""
                )));
            }

            let path = project.resolved_path(&self.projects_dir)?;
            project.name = Some(name);
            project.path = Some(path);

            match project
                .default_session
                .as_ref()
                .unwrap_or(&ProjectSessionConfig::Template(
                    global_default_session.clone(),
                )) {
                ProjectSessionConfig::Template(template_name) => {
                    if !self.sessions.contains_key(template_name) {
                        return Err(ConfigError::Validation(format!(
                            "Project \"{}\" references unknown session template \"{template_name}\"",
                            project.name.as_deref().unwrap_or("<unknown>")
                        )));
                    }
                }
                ProjectSessionConfig::Inline(session) => {
                    session.validate(&format!(
                        "Project \"{}\" inline default_session",
                        project.name.as_deref().unwrap_or("<unknown>")
                    ))?;
                }
            }
        }

        Ok(())
    }

    pub fn session_template(&self, name: &str) -> Option<&SessionConfig> {
        self.sessions.get(name)
    }

    pub fn project(&self, name: &str) -> Option<ResolvedProject> {
        self.projects.iter().find_map(|project| {
            let resolved_name = project.resolved_name().ok()?;
            if resolved_name != name {
                return None;
            }

            Some(ResolvedProject {
                repository: project.repository.clone(),
                name: resolved_name,
                path: project.resolved_path(&self.projects_dir).ok()?,
                default_session: project.resolved_default_session(self),
            })
        })
    }

    pub fn project_session_template<'a>(
        &'a self,
        project: &'a ResolvedProject,
        session_override: Option<&str>,
    ) -> Option<&'a SessionConfig> {
        if let Some(template_name) = session_override {
            return self.session_template(template_name);
        }

        match &project.default_session {
            ProjectSessionConfig::Template(template_name) => self.session_template(template_name),
            ProjectSessionConfig::Inline(session) => Some(session),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProject {
    repository: String,
    name: String,
    path: PathBuf,
    default_session: ProjectSessionConfig,
}

/// Replaces '~' with the contents of $HOME
fn expand_home(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~")
        && let Some(home) = std::env::home_dir()
    {
        return home.join(stripped);
    }
    path.to_path_buf()
}

fn default_projects_dir() -> PathBuf {
    PathBuf::from("~/Projects")
}

fn default_default_session() -> String {
    "default".to_owned()
}

fn repository_basename(repository: &str) -> Result<String, ConfigError> {
    let trimmed = repository.trim().trim_end_matches('/');
    let basename = trimmed.rsplit(['/', ':']).next().unwrap_or(trimmed);
    let basename = basename.strip_suffix(".git").unwrap_or(basename).to_owned();

    validate_project_name(&basename)?;
    Ok(basename)
}

fn validate_project_name(name: &str) -> Result<(), ConfigError> {
    if name.trim().is_empty() || name.contains(':') {
        return Err(ConfigError::Validation(format!(
            "\"{name}\" is not a valid project name"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> WindowConfig {
        WindowConfig { commands: vec![] }
    }

    fn session() -> SessionConfig {
        SessionConfig {
            windows: vec![window()],
        }
    }

    fn config_with_default() -> Config {
        Config {
            projects_dir: PathBuf::from("~/Projects"),
            default_session: "default".to_owned(),
            sessions: HashMap::from([("default".to_owned(), session())]),
            projects: vec![],
        }
    }

    #[test]
    fn expands_projects_dir_and_project_path() {
        let home = std::env::home_dir().expect("HOME should be set for tests");
        let mut config = config_with_default();
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: Some(PathBuf::from("~/src/repo")),
            default_session: None,
        });

        config.validate_and_normalize().unwrap();

        assert_eq!(config.projects_dir, home.join("Projects"));
        assert_eq!(config.projects[0].path, Some(home.join("src/repo")));
    }

    #[test]
    fn derives_project_name_from_repository_basename() {
        for (repository, expected) in [
            ("git@github.com:owner/repo.git", "repo"),
            ("https://github.com/owner/repo.git", "repo"),
            ("https://github.com/owner/repo", "repo"),
        ] {
            let project = ProjectConfig {
                repository: repository.to_owned(),
                name: None,
                path: None,
                default_session: None,
            };

            assert_eq!(project.resolved_name().unwrap(), expected);
        }
    }

    #[test]
    fn missing_global_default_session_fails_validation() {
        let mut config = Config {
            projects_dir: PathBuf::from("~/Projects"),
            default_session: "missing".to_owned(),
            sessions: HashMap::from([("default".to_owned(), session())]),
            projects: vec![],
        };

        assert!(config.validate_and_normalize().is_err());
    }

    #[test]
    fn empty_session_template_windows_fail_validation() {
        let mut config = Config {
            projects_dir: PathBuf::from("~/Projects"),
            default_session: "default".to_owned(),
            sessions: HashMap::from([("default".to_owned(), SessionConfig { windows: vec![] })]),
            projects: vec![],
        };

        assert!(config.validate_and_normalize().is_err());
    }

    #[test]
    fn duplicate_resolved_project_names_fail_validation() {
        let mut config = config_with_default();
        config.projects = vec![
            ProjectConfig {
                repository: "git@github.com:owner/repo.git".to_owned(),
                name: None,
                path: None,
                default_session: None,
            },
            ProjectConfig {
                repository: "https://github.com/other/repo.git".to_owned(),
                name: None,
                path: None,
                default_session: None,
            },
        ];

        assert!(config.validate_and_normalize().is_err());
    }

    #[test]
    fn project_default_session_must_exist() {
        let mut config = config_with_default();
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: Some(ProjectSessionConfig::Template("missing".to_owned())),
        });

        assert!(config.validate_and_normalize().is_err());
    }

    #[test]
    fn project_default_session_can_be_inline() {
        let mut config = config_with_default();
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: Some(ProjectSessionConfig::Inline(SessionConfig {
                windows: vec![WindowConfig {
                    commands: vec!["cargo check".to_owned()],
                }],
            })),
        });

        config.validate_and_normalize().unwrap();
        let project = config.project("repo").unwrap();

        assert!(matches!(
            project.default_session,
            ProjectSessionConfig::Inline(_)
        ));
    }

    #[test]
    fn inline_project_default_session_must_have_windows() {
        let mut config = config_with_default();
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: Some(ProjectSessionConfig::Inline(SessionConfig {
                windows: vec![],
            })),
        });

        assert!(config.validate_and_normalize().is_err());
    }

    #[test]
    fn project_path_defaults_to_projects_dir_and_project_name() {
        let mut config = config_with_default();
        config.projects_dir = PathBuf::from("/tmp/projects");
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: None,
        });

        config.validate_and_normalize().unwrap();

        assert_eq!(
            config.projects[0].path,
            Some(PathBuf::from("/tmp/projects/repo"))
        );
    }

    #[test]
    fn old_rooted_session_schema_is_rejected() {
        let err = serde_json::from_str::<Config>(
            r#"{
                "default_session": "default",
                "sessions": {
                    "default": {
                        "root": "/tmp",
                        "windows": [{ "commands": [] }]
                    }
                }
            }"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unknown field `root`"));
    }
}
