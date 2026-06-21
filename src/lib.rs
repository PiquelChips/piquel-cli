//! Data types and helpers for the `piquelcli` command-line tool.

/// Command-line parsing and top-level dispatch.
pub mod cli;
/// JSON config loading and global config access.
pub mod config;
/// Interactive fuzzy selection helpers.
pub mod fzf;
/// Git worktree discovery helpers.
pub mod git;
/// Integration helpers for invoking tmux.
pub mod tmux;

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::config::ConfigError;

/// Commands to send to a tmux window after creating it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    #[serde(default)]
    commands: Vec<String>,
}

/// Configuration for a tmux session template.
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

/// Configuration for a repository-backed project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    repository: String,
    name: Option<String>,
    path: Option<PathBuf>,
    default_session: Option<ProjectSessionConfig>,
}

impl ProjectConfig {
    /// Returns the configured project name, or derives one from the repository URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured or derived name is not valid.
    pub fn resolved_name(&self) -> Result<String, ConfigError> {
        let name = match &self.name {
            Some(name) => name.clone(),
            None => repository_basename(&self.repository)?,
        };

        validate_project_name(&name)?;
        Ok(name)
    }

    /// Returns the configured project path, or derives one under `projects_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the project name cannot be resolved.
    pub fn resolved_path(&self, projects_dir: &Path) -> Result<PathBuf, ConfigError> {
        match &self.path {
            Some(path) => Ok(expand_home(path)),
            None => Ok(projects_dir.join(self.resolved_name()?)),
        }
    }

    /// Returns the project default session config, falling back to the global default.
    #[must_use]
    pub fn resolved_default_session(&self, config: &Config) -> ProjectSessionConfig {
        self.default_session
            .clone()
            .unwrap_or_else(|| ProjectSessionConfig::Template(config.default_session.clone()))
    }
}

/// A project's default session, either by template name or inline template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ProjectSessionConfig {
    /// Name of a global session template.
    Template(String),
    /// Inline session template defined on the project.
    Inline(SessionConfig),
}

/// Complete JSON configuration for the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_projects_dir")]
    projects_dir: PathBuf,
    #[serde(default = "default_worktrees_dir")]
    worktrees_dir: PathBuf,
    #[serde(default = "default_default_session")]
    default_session: String,
    #[serde(default)]
    sessions: HashMap<String, SessionConfig>,
    #[serde(default)]
    projects: Vec<ProjectConfig>,
}

impl Config {
    /// Validates config semantics and expands paths in place.
    ///
    /// # Errors
    ///
    /// Returns an error if session templates, project names, project paths, or
    /// default-session references are invalid.
    pub fn validate_and_normalize(&mut self) -> Result<(), ConfigError> {
        self.projects_dir = expand_home(&self.projects_dir);
        self.worktrees_dir = expand_home(&self.worktrees_dir);

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

    /// Returns the named global session template.
    #[must_use]
    pub fn session_template(&self, name: &str) -> Option<&SessionConfig> {
        self.sessions.get(name)
    }

    /// Returns a normalized project by name.
    #[must_use]
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

    /// Returns the session template that should be used for `project`.
    #[must_use]
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

/// Project configuration after name, path, and default session resolution.
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

fn default_worktrees_dir() -> PathBuf {
    PathBuf::from("~/.piquel/worktrees")
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
            worktrees_dir: PathBuf::from("~/.piquel/worktrees"),
            default_session: "default".to_owned(),
            sessions: HashMap::from([("default".to_owned(), session())]),
            projects: vec![],
        }
    }

    #[test]
    fn expands_projects_dir_worktrees_dir_and_project_path() {
        let home = std::env::home_dir().expect("HOME should be set for tests");
        let mut config = config_with_default();
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: Some(PathBuf::from("~/src/repo")),
            default_session: None,
        });

        config
            .validate_and_normalize()
            .expect("config should validate");

        assert_eq!(config.projects_dir, home.join("Projects"));
        assert_eq!(config.worktrees_dir, home.join(".piquel/worktrees"));
        assert_eq!(config.projects[0].path, Some(home.join("src/repo")));
    }

    #[test]
    fn worktrees_dir_defaults_and_expands_from_json_config() {
        let home = std::env::home_dir().expect("HOME should be set for tests");
        let mut config = serde_json::from_str::<Config>(
            r#"{
                "default_session": "default",
                "sessions": {
                    "default": { "windows": [{ "commands": [] }] }
                }
            }"#,
        )
        .expect("config should parse");

        config
            .validate_and_normalize()
            .expect("config should validate");

        assert_eq!(config.worktrees_dir, home.join(".piquel/worktrees"));
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

            assert_eq!(
                project
                    .resolved_name()
                    .expect("project name should resolve"),
                expected
            );
        }
    }

    #[test]
    fn missing_global_default_session_fails_validation() {
        let mut config = Config {
            projects_dir: PathBuf::from("~/Projects"),
            worktrees_dir: PathBuf::from("~/.piquel/worktrees"),
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
            worktrees_dir: PathBuf::from("~/.piquel/worktrees"),
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

        config
            .validate_and_normalize()
            .expect("config should validate");
        let project = config.project("repo").expect("project should resolve");

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

        config
            .validate_and_normalize()
            .expect("config should validate");

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
        .expect_err("old rooted session schema should be rejected");

        assert!(err.to_string().contains("unknown field `root`"));
    }
}
