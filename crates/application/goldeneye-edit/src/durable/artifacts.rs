use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use goldeneye_domain::{ContentHash, Generation, ProjectId, ProjectRelativePath};
use goldeneye_ports::EditOperationId;

use super::DurableEditError;
use crate::path_auth::AuthorizedPath;

static ACTIVE_TARGETS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

#[derive(Debug)]
pub(super) struct ArtifactPaths {
    pub(super) temp_relative: ProjectRelativePath,
    pub(super) backup_relative: ProjectRelativePath,
    pub(super) temp_absolute: PathBuf,
    pub(super) backup_absolute: PathBuf,
}

impl ArtifactPaths {
    pub(super) fn new(
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
    ) -> Result<Self, DurableEditError> {
        let digest = ContentHash::of(operation_id.as_str()).to_string();
        let key = &digest[..20];
        let path = authorized.relative_path().as_str();
        let prefix = path
            .rsplit_once('/')
            .map_or(String::new(), |(parent, _)| format!("{parent}/"));
        let temp_relative = ProjectRelativePath::new(format!("{prefix}.goldeneye-edit-{key}.tmp"))?;
        let backup_relative =
            ProjectRelativePath::new(format!("{prefix}.goldeneye-edit-{key}.bak"))?;
        let temp_absolute = join_relative(authorized.project_root(), &temp_relative);
        let backup_absolute = join_relative(authorized.project_root(), &backup_relative);
        Ok(Self {
            temp_relative,
            backup_relative,
            temp_absolute,
            backup_absolute,
        })
    }
}

#[derive(Debug)]
pub(super) struct TargetLease {
    key: PathBuf,
}

impl TargetLease {
    pub(super) fn acquire(path: &Path) -> Result<Self, DurableEditError> {
        let key = target_key(path);
        let targets = ACTIVE_TARGETS.get_or_init(|| Mutex::new(BTreeSet::new()));
        let mut guard = targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !guard.insert(key.clone()) {
            return Err(DurableEditError::TargetBusy(path.to_path_buf()));
        }
        Ok(Self { key })
    }
}

impl Drop for TargetLease {
    fn drop(&mut self) {
        if let Some(targets) = ACTIVE_TARGETS.get() {
            targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.key);
        }
    }
}

#[cfg(windows)]
fn target_key(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn target_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub(super) fn graph_changes(
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
) -> super::GraphChanges {
    super::GraphChanges {
        added: after.difference(before).count(),
        removed: before.difference(after).count(),
        retained: before.intersection(after).count(),
    }
}

pub(super) fn changed_graph_identities(
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
) -> Vec<String> {
    let mut identities = before.union(after).cloned().collect::<Vec<_>>();
    identities.truncate(64);
    identities
}

pub(super) fn ensure_generation(
    expected: Generation,
    actual: Generation,
) -> Result<(), DurableEditError> {
    if expected != actual {
        return Err(DurableEditError::StaleGeneration { expected, actual });
    }
    Ok(())
}

pub(super) fn next_generation(
    project_id: &ProjectId,
    generation: Generation,
) -> Result<Generation, DurableEditError> {
    generation
        .value()
        .checked_add(1)
        .map(Generation::new)
        .ok_or_else(|| DurableEditError::GenerationOverflow(project_id.clone()))
}

pub(super) fn required_hash(
    hash: Option<ContentHash>,
    operation_id: &EditOperationId,
    label: &str,
) -> Result<ContentHash, DurableEditError> {
    hash.ok_or_else(|| DurableEditError::RecoveryRequired {
        operation_id: operation_id.as_str().to_owned(),
        reason: format!("journal has no {label} hash"),
    })
}

fn project_relative_from_absolute(
    project_root: &Path,
    path: &Path,
) -> Result<ProjectRelativePath, DurableEditError> {
    let relative = path
        .strip_prefix(project_root)
        .map_err(|source| DurableEditError::Io {
            operation: "making path project-relative",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, source),
        })?;
    let mut segments = Vec::new();
    for component in relative.components() {
        let value = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| DurableEditError::Io {
                operation: "encoding project-relative path",
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"),
            })?;
        segments.push(value);
    }
    Ok(ProjectRelativePath::new(segments.join("/"))?)
}

pub(super) fn planned_missing_parents(
    authorized: &AuthorizedPath,
) -> Result<Vec<ProjectRelativePath>, DurableEditError> {
    let segments = authorized.relative_path().as_str().split('/');
    let parent_count = segments.clone().count().saturating_sub(1);
    let mut current = authorized.project_root().to_path_buf();
    let mut missing = Vec::new();
    for segment in segments.take(parent_count) {
        current.push(segment);
        if !super::path_present(&current)? {
            missing.push(project_relative_from_absolute(
                authorized.project_root(),
                &current,
            )?);
        }
    }
    Ok(missing)
}

pub(super) fn join_relative(root: &Path, path: &ProjectRelativePath) -> PathBuf {
    path.as_str()
        .split('/')
        .fold(root.to_path_buf(), |current, segment| current.join(segment))
}
