//! Data types and helpers for the `piquelcli` command-line tool.

/// Machine interaction backend abstraction.
pub mod backend;
/// Command-line parsing and top-level dispatch.
pub mod cli;
/// JSON config loading.
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

use crate::{backend::Backend, config::ConfigError};

/// Commands to send to a tmux window after creating it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    name: Option<String>,
    #[serde(default)]
    commands: Vec<String>,
}

impl WindowConfig {
    fn validate(&self, session_name: &str, index: usize) -> Result<(), config::ConfigError> {
        if self
            .name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(ConfigError::Validation(format!(
                "Window {} in session template \"{session_name}\" has an empty name. If you don't want to specify a name, remove the field.",
                index + 1
            )));
        }

        Ok(())
    }
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

        for (index, window) in self.windows.iter().enumerate() {
            window.validate(name, index)?;
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
    pub fn resolved_path(
        &self,
        backend: &dyn Backend,
        projects_dir: &Path,
    ) -> Result<PathBuf, ConfigError> {
        match &self.path {
            Some(path) => Ok(backend.expand_home(path)?),
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
    pub fn validate_and_normalize(&mut self, backend: &dyn Backend) -> Result<(), ConfigError> {
        self.projects_dir = backend.expand_home(&self.projects_dir)?;
        self.worktrees_dir = backend.expand_home(&self.worktrees_dir)?;

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

            let path = project.resolved_path(backend, &self.projects_dir)?;
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

            let path = project
                .path
                .clone()
                .unwrap_or_else(|| self.projects_dir.join(&resolved_name));

            Some(ResolvedProject {
                repository: project.repository.clone(),
                name: resolved_name,
                path,
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
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || name.contains(':')
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(ConfigError::Validation(format!(
            "\"{name}\" is not a valid project name"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::LocalBackend;

    fn window() -> WindowConfig {
        WindowConfig {
            name: None,
            commands: vec![],
        }
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

    fn backend() -> LocalBackend {
        LocalBackend
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
            .validate_and_normalize(&backend())
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
            .validate_and_normalize(&backend())
            .expect("config should validate");

        assert_eq!(config.worktrees_dir, home.join(".piquel/worktrees"));
    }

    #[test]
    fn derives_project_name_from_repository_basename() {
        for (repository, expected) in [
            ("git@github.com:owner/repo.git", "repo"),
            ("https://github.com/owner/repo.git", "repo"),
            ("https://github.com/owner/repo", "repo"),
            ("ssh://git@example.com/owner/repo.git/", "repo"),
            ("  https://github.com/owner/repo.git///  ", "repo"),
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
    fn invalid_repository_basename_fails_project_name_resolution() {
        for repository in ["", "   ", "git@example.com:owner/.."] {
            let project = ProjectConfig {
                repository: repository.to_owned(),
                name: None,
                path: None,
                default_session: None,
            };

            assert!(
                project.resolved_name().is_err(),
                "{repository:?} should not resolve to a safe project name"
            );
        }
    }

    #[test]
    fn project_names_must_be_safe_path_segments() {
        for name in [
            "",
            "   ",
            ".",
            "..",
            "owner/repo",
            r"owner\repo",
            "repo:name",
        ] {
            let project = ProjectConfig {
                repository: "https://github.com/owner/repo.git".to_owned(),
                name: Some(name.to_owned()),
                path: None,
                default_session: None,
            };

            assert!(
                project.resolved_name().is_err(),
                "{name:?} should be rejected"
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

        assert!(config.validate_and_normalize(&backend()).is_err());
    }

    #[test]
    fn invalid_session_template_names_fail_validation() {
        for name in ["", "   ", "project:branch"] {
            let mut config = Config {
                projects_dir: PathBuf::from("~/Projects"),
                worktrees_dir: PathBuf::from("~/.piquel/worktrees"),
                default_session: name.to_owned(),
                sessions: HashMap::from([(name.to_owned(), session())]),
                projects: vec![],
            };

            assert!(
                config.validate_and_normalize(&backend()).is_err(),
                "{name:?} should be rejected as a template name"
            );
        }
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

        assert!(config.validate_and_normalize(&backend()).is_err());
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

        assert!(config.validate_and_normalize(&backend()).is_err());
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

        assert!(config.validate_and_normalize(&backend()).is_err());
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
                    name: None,
                    commands: vec!["cargo check".to_owned()],
                }],
            })),
        });

        config
            .validate_and_normalize(&backend())
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

        assert!(config.validate_and_normalize(&backend()).is_err());
    }

    #[test]
    fn blank_window_names_fail_validation() {
        let mut config = config_with_default();
        config.sessions.insert(
            "default".to_owned(),
            SessionConfig {
                windows: vec![WindowConfig {
                    name: Some("   ".to_owned()),
                    commands: vec![],
                }],
            },
        );

        assert!(config.validate_and_normalize(&backend()).is_err());
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
            .validate_and_normalize(&backend())
            .expect("config should validate");

        assert_eq!(
            config.projects[0].path,
            Some(PathBuf::from("/tmp/projects/repo"))
        );
    }

    #[test]
    fn project_lookup_returns_normalized_project_data() {
        let mut config = config_with_default();
        config.projects_dir = PathBuf::from("/tmp/projects");
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: Some(ProjectSessionConfig::Template("default".to_owned())),
        });

        config
            .validate_and_normalize(&backend())
            .expect("config should validate");
        let project = config.project("repo").expect("project should exist");

        assert_eq!(project.repository, "git@github.com:owner/repo.git");
        assert_eq!(project.name, "repo");
        assert_eq!(project.path, PathBuf::from("/tmp/projects/repo"));
        assert_eq!(
            project.default_session,
            ProjectSessionConfig::Template("default".to_owned())
        );
    }

    #[test]
    fn project_session_template_prefers_override_then_project_default() {
        let mut config = config_with_default();
        config.sessions.insert(
            "rust".to_owned(),
            SessionConfig {
                windows: vec![WindowConfig {
                    name: Some("rust".to_owned()),
                    commands: vec!["cargo check".to_owned()],
                }],
            },
        );
        config.projects.push(ProjectConfig {
            repository: "git@github.com:owner/repo.git".to_owned(),
            name: None,
            path: None,
            default_session: Some(ProjectSessionConfig::Template("rust".to_owned())),
        });
        config
            .validate_and_normalize(&backend())
            .expect("config should validate");
        let project = config.project("repo").expect("project should exist");

        let default = config
            .project_session_template(&project, None)
            .expect("project default should resolve");
        let override_template = config
            .project_session_template(&project, Some("default"))
            .expect("override should resolve");

        assert_eq!(default.windows[0].name.as_deref(), Some("rust"));
        assert_eq!(override_template.windows[0].name, None);
        assert!(
            config
                .project_session_template(&project, Some("missing"))
                .is_none()
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
