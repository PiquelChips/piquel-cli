use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, bail};

use crate::{
    MachineConfig, ResolvedProject, fzf, git,
    mount::{self, MountRecord, MountStatus, MountedWorkspace, SshTarget, WorkspaceMount},
};

use super::State;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceChoice {
    item: String,
    workspace: MountedWorkspace,
    remote_path: PathBuf,
}

impl State {
    /// Mounts a remote project or existing worktree with SSHFS.
    ///
    /// # Errors
    ///
    /// Returns an error if the machine or project/worktree cannot
    /// be resolved, SSHFS fails, or local state cannot be updated.
    pub fn mount(
        &self,
        machine_name: &str,
        project_name: Option<&str>,
        worktree: Option<&str>,
        repair: bool,
    ) -> Result<()> {
        let machine = self.configured_machine(machine_name)?;
        let explicit_project = project_name.is_some();
        let project = match project_name {
            Some(project_name) => self.configured_project(project_name)?,
            None => self
                .pick_project()?
                .ok_or_else(|| anyhow!("No project selected"))?,
        };

        let target = ssh_target(machine);
        Self::ensure_mountable_project_path(&target, &project)?;

        let (workspace, remote_path) = if let Some(branch) = worktree {
            let worktrees = Self::list_mount_worktrees(&target, &project.path)?;
            let worktree = git::find_worktree(worktrees, &project.path, branch)?;
            (
                MountedWorkspace::Branch {
                    branch: branch.to_owned(),
                },
                worktree.path,
            )
        } else if explicit_project {
            (MountedWorkspace::Main, project.path.clone())
        } else {
            let choice = Self::pick_workspace(&project, &target)?
                .ok_or_else(|| anyhow!("No workspace selected"))?;
            (choice.workspace, choice.remote_path)
        };

        let record = self.mount_record(machine, &project, workspace, remote_path);

        if repair {
            self.unmount_record_if_mounted(&record)?;
        }

        mount::mount_workspace(&record).map_err(|error| {
            anyhow!("Failed to mount {}: {error}", record.remote_path.display())
        })?;

        let mut state = mount::load_state(self.config.mounts_dir())?;
        mount::upsert_record(&mut state, record.clone());
        mount::save_state(self.config.mounts_dir(), &state)?;

        println!("{}", record.mount_path.display());
        Ok(())
    }

    /// Lists known managed mounts for the configured machine.
    ///
    /// # Errors
    ///
    /// Returns an error if the machine is unknown or local state/mount table
    /// cannot be read.
    pub fn mounts(&self, machine_name: &str) -> Result<()> {
        let machine = self.configured_machine(machine_name)?;
        for workspace_mount in mount::list_known_mounts(self.config.mounts_dir(), machine.name())? {
            println!("{}", format_mount_line(&workspace_mount));
        }
        Ok(())
    }

    /// Unmounts managed SSHFS mounts.
    ///
    /// # Errors
    ///
    /// Returns an error if the machine is unknown, selection fails, unmounting
    /// fails, or local state cannot be updated.
    pub fn unmount(
        &self,
        machine_name: &str,
        project_name: Option<&str>,
        worktree: Option<&str>,
        all: bool,
        stale: bool,
        repair: bool,
    ) -> Result<()> {
        let machine = self.configured_machine(machine_name)?;

        if stale {
            let removed = mount::prune_stale(self.config.mounts_dir(), machine.name())?;
            println!("Removed {removed} stale mount record(s)");
            return Ok(());
        }

        let known = mount::list_known_mounts(self.config.mounts_dir(), machine.name())?;
        let selected = if all {
            known
        } else if let Some(project_name) = project_name {
            filter_mounts_for_project(known, project_name, worktree)
        } else {
            Self::pick_mount(known)?.map_or_else(Vec::new, |workspace_mount| vec![workspace_mount])
        };

        if selected.is_empty() {
            return Ok(());
        }

        let mut mount_state = mount::load_state(self.config.mounts_dir())?;
        for workspace_mount in selected {
            if workspace_mount.status == MountStatus::Mounted
                || (repair && workspace_mount.status == MountStatus::Recorded)
            {
                mount::unmount_workspace(&workspace_mount.record.mount_path).map_err(|error| {
                    anyhow!(
                        "Failed to unmount {}: {error}",
                        workspace_mount.record.mount_path.display()
                    )
                })?;
            }
            mount::remove_record(&mut mount_state, &workspace_mount.record.mount_path);
        }
        mount::save_state(self.config.mounts_dir(), &mount_state)?;
        Ok(())
    }

    fn configured_machine(&self, machine_name: &str) -> Result<&MachineConfig> {
        self.config
            .machine(machine_name)
            .ok_or_else(|| anyhow!("Machine \"{machine_name}\" is not configured"))
    }

    fn configured_project(&self, project_name: &str) -> Result<ResolvedProject> {
        self.config
            .project(project_name)
            .ok_or_else(|| anyhow!("Project \"{project_name}\" is not configured"))
    }

    fn pick_project(&self) -> Result<Option<ResolvedProject>> {
        let mut projects = self
            .config
            .projects
            .iter()
            .filter_map(|project| project.resolved_name().ok())
            .collect::<Vec<_>>();
        projects.sort();
        projects.dedup();

        let Some(selection) = fzf::select(projects, "project> ")? else {
            return Ok(None);
        };

        Ok(Some(self.configured_project(&selection)?))
    }

    fn pick_workspace(
        project: &ResolvedProject,
        target: &SshTarget,
    ) -> Result<Option<WorkspaceChoice>> {
        let worktrees = Self::list_mount_worktrees(target, &project.path)?;
        let choices = workspace_choices(project, &worktrees);
        let mut targets = choices
            .iter()
            .map(|choice| (choice.item.clone(), choice.clone()))
            .collect::<HashMap<_, _>>();
        let items = choices
            .into_iter()
            .map(|choice| choice.item)
            .collect::<Vec<_>>();

        let Some(selection) = fzf::select(items, "workspace> ")? else {
            return Ok(None);
        };

        targets
            .remove(&selection)
            .map(Some)
            .ok_or_else(|| anyhow!("Selected unknown workspace \"{selection}\""))
    }

    fn pick_mount(mounts: Vec<WorkspaceMount>) -> Result<Option<WorkspaceMount>> {
        let choices = mounts
            .into_iter()
            .map(|workspace_mount| (format_mount_line(&workspace_mount), workspace_mount))
            .collect::<Vec<_>>();
        let mut targets = choices.iter().cloned().collect::<HashMap<_, _>>();
        let items = choices
            .into_iter()
            .map(|(item, _)| item)
            .collect::<Vec<_>>();

        let Some(selection) = fzf::select(items, "mount> ")? else {
            return Ok(None);
        };

        targets
            .remove(&selection)
            .map(Some)
            .ok_or_else(|| anyhow!("Selected unknown mount \"{selection}\""))
    }

    fn ensure_mountable_project_path(target: &SshTarget, project: &ResolvedProject) -> Result<()> {
        if !mount::remote_path_exists(target, &project.path)? {
            bail!(
                "Project \"{}\" path {} does not exist",
                project.name,
                project.path.display()
            );
        }

        if !mount::remote_path_is_dir(target, &project.path)? {
            bail!(
                "Project \"{}\" path {} is not a directory",
                project.name,
                project.path.display()
            );
        }

        Ok(())
    }

    fn list_mount_worktrees(target: &SshTarget, project_path: &Path) -> Result<Vec<git::Worktree>> {
        Ok(mount::remote_worktrees(target, project_path)?)
    }

    fn mount_record(
        &self,
        machine: &MachineConfig,
        project: &ResolvedProject,
        workspace: MountedWorkspace,
        remote_path: PathBuf,
    ) -> MountRecord {
        let mount_path = mount::local_mount_path(
            self.config.mounts_dir(),
            machine.name(),
            &project.name,
            &workspace,
            &remote_path,
        );

        MountRecord {
            machine: machine.name().to_owned(),
            project: project.name.clone(),
            workspace,
            target: ssh_target(machine),
            remote_path,
            mount_path,
        }
    }

    fn unmount_record_if_mounted(&self, record: &MountRecord) -> Result<()> {
        let known = mount::list_known_mounts(self.config.mounts_dir(), &record.machine)?;

        if let Some(existing) = known.into_iter().find(|workspace_mount| {
            workspace_mount.record.project == record.project
                && workspace_mount.record.workspace == record.workspace
        }) && existing.status == MountStatus::Mounted
        {
            mount::unmount_workspace(&existing.record.mount_path)?;
        }

        Ok(())
    }
}

fn workspace_choices(
    project: &ResolvedProject,
    worktrees: &[git::Worktree],
) -> Vec<WorkspaceChoice> {
    let mut choices = vec![WorkspaceChoice {
        item: "main".to_owned(),
        workspace: MountedWorkspace::Main,
        remote_path: project.path.clone(),
    }];

    for worktree in worktrees {
        if worktree.path == project.path {
            continue;
        }

        if let Some(branch) = &worktree.branch {
            choices.push(WorkspaceChoice {
                item: branch.clone(),
                workspace: MountedWorkspace::Branch {
                    branch: branch.clone(),
                },
                remote_path: worktree.path.clone(),
            });
        } else {
            let label = worktree.path.file_name().map_or_else(
                || "detached".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            choices.push(WorkspaceChoice {
                item: format!("detached:{label} {}", worktree.path.display()),
                workspace: MountedWorkspace::Detached { label },
                remote_path: worktree.path.clone(),
            });
        }
    }

    choices.sort_by(|left, right| workspace_sort_key(left).cmp(&workspace_sort_key(right)));
    choices
}

fn ssh_target(machine: &MachineConfig) -> SshTarget {
    SshTarget {
        username: machine.username().to_owned(),
        host: machine.address().to_owned(),
    }
}

fn workspace_sort_key(choice: &WorkspaceChoice) -> (u8, &str) {
    match &choice.workspace {
        MountedWorkspace::Main => (0, ""),
        MountedWorkspace::Branch { branch } => (1, branch.as_str()),
        MountedWorkspace::Detached { label } => (2, label.as_str()),
    }
}

fn filter_mounts_for_project(
    mounts: Vec<WorkspaceMount>,
    project_name: &str,
    worktree: Option<&str>,
) -> Vec<WorkspaceMount> {
    mounts
        .into_iter()
        .filter(|workspace_mount| {
            workspace_mount.record.project == project_name
                && match (&workspace_mount.record.workspace, worktree) {
                    (MountedWorkspace::Main, None) => true,
                    (MountedWorkspace::Branch { branch }, Some(requested)) => branch == requested,
                    _ => false,
                }
        })
        .collect()
}

fn format_mount_line(workspace_mount: &WorkspaceMount) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        status_label(workspace_mount.status),
        workspace_label(&workspace_mount.record.workspace),
        workspace_mount.record.project,
        workspace_mount.record.mount_path.display()
    )
}

fn status_label(status: MountStatus) -> &'static str {
    match status {
        MountStatus::Recorded => "recorded",
        MountStatus::Mounted => "mounted",
        MountStatus::Missing => "missing",
    }
}

fn workspace_label(workspace: &MountedWorkspace) -> String {
    match workspace {
        MountedWorkspace::Main => "main".to_owned(),
        MountedWorkspace::Branch { branch } => format!("branch:{branch}"),
        MountedWorkspace::Detached { label } => format!("detached:{label}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ResolvedProject {
        ResolvedProject {
            repository: "https://github.com/owner/alpha.git".to_owned(),
            name: "alpha".to_owned(),
            path: PathBuf::from("/repo/alpha"),
            default_session: crate::ProjectSessionConfig::Template("default".to_owned()),
        }
    }

    #[test]
    fn workspace_choices_include_main_branches_and_detached() {
        let choices = workspace_choices(
            &project(),
            &[
                git::Worktree {
                    path: PathBuf::from("/repo/alpha"),
                    branch: Some("main".to_owned()),
                },
                git::Worktree {
                    path: PathBuf::from("/repo/alpha-feature"),
                    branch: Some("feature/x".to_owned()),
                },
                git::Worktree {
                    path: PathBuf::from("/repo/alpha-detached"),
                    branch: None,
                },
            ],
        );

        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.item.as_str())
                .collect::<Vec<_>>(),
            vec![
                "main",
                "feature/x",
                "detached:alpha-detached /repo/alpha-detached"
            ]
        );
    }

    #[test]
    fn project_filter_defaults_to_main_mount() {
        let mounts = vec![
            workspace_mount("alpha", MountedWorkspace::Main),
            workspace_mount(
                "alpha",
                MountedWorkspace::Branch {
                    branch: "feature/x".to_owned(),
                },
            ),
        ];

        let filtered = filter_mounts_for_project(mounts, "alpha", None);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].record.workspace, MountedWorkspace::Main);
    }

    fn workspace_mount(project: &str, workspace: MountedWorkspace) -> WorkspaceMount {
        WorkspaceMount {
            record: MountRecord {
                machine: "worker".to_owned(),
                project: project.to_owned(),
                workspace,
                target: SshTarget {
                    username: "me".to_owned(),
                    host: "host".to_owned(),
                },
                remote_path: PathBuf::from("/remote"),
                mount_path: PathBuf::from("/mount"),
            },
            status: MountStatus::Mounted,
        }
    }
}
