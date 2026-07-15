use std::{
    fs, io,
    path::{Path, PathBuf},
};

use goldeneye_domain::ProjectRelativePath;

use super::{PathAuthorizationError, RESERVED_SEGMENTS};

pub(super) fn parse_relative_path(
    value: &str,
) -> Result<ProjectRelativePath, PathAuthorizationError> {
    let relative_path = ProjectRelativePath::new(value.to_owned()).map_err(|source| {
        PathAuthorizationError::InvalidRelativePath {
            path: value.to_owned(),
            source,
        }
    })?;
    for segment in value.split('/') {
        validate_segment(value, segment)?;
    }
    Ok(relative_path)
}

fn validate_segment(path: &str, segment: &str) -> Result<(), PathAuthorizationError> {
    if segment.contains(':') {
        return Err(PathAuthorizationError::UnsupportedPlatformComponent {
            path: path.to_owned(),
            segment: segment.to_owned(),
        });
    }
    if RESERVED_SEGMENTS
        .iter()
        .any(|reserved| segment.eq_ignore_ascii_case(reserved))
    {
        return Err(PathAuthorizationError::ReservedMetadata {
            path: path.to_owned(),
            segment: segment.to_owned(),
        });
    }
    Ok(())
}

pub(super) fn ensure_project_allowed(
    project_root: &Path,
    allowed_roots: &[PathBuf],
) -> Result<(), PathAuthorizationError> {
    if allowed_roots
        .iter()
        .any(|allowed_root| project_root.starts_with(allowed_root))
    {
        Ok(())
    } else {
        Err(PathAuthorizationError::ProjectOutsideAllowedRoots {
            path: project_root.to_path_buf(),
        })
    }
}

pub(super) fn validate_existing_ancestry(
    project_root: &Path,
    destination: &Path,
) -> Result<(), PathAuthorizationError> {
    let ancestor = nearest_existing_ancestor(destination)?;
    let resolved = canonicalize("canonicalize existing path ancestry", &ancestor)?;
    if !resolved.starts_with(project_root) {
        return Err(PathAuthorizationError::PathEscapesProject {
            path: destination.to_path_buf(),
            resolved,
        });
    }
    if ancestor != destination {
        require_directory("inspect existing path ancestry", &resolved).map_err(|error| {
            if matches!(
                error,
                PathAuthorizationError::ExistingAncestorNotDirectory { .. }
            ) {
                PathAuthorizationError::ExistingAncestorNotDirectory { path: ancestor }
            } else {
                error
            }
        })?;
    }
    Ok(())
}

pub(super) fn require_confined_directory(
    project_root: &Path,
    path: &Path,
) -> Result<(), PathAuthorizationError> {
    let resolved = canonicalize("canonicalize parent directory", path)?;
    if !resolved.starts_with(project_root) {
        return Err(PathAuthorizationError::PathEscapesProject {
            path: path.to_path_buf(),
            resolved,
        });
    }
    require_directory("inspect parent directory", &resolved)
}

fn nearest_existing_ancestor(path: &Path) -> Result<PathBuf, PathAuthorizationError> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(_) => return Ok(current),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let Some(parent) = current.parent() else {
                    return Err(io_failure("locate existing path ancestry", path, source));
                };
                current = parent.to_path_buf();
            }
            Err(source) => {
                return Err(io_failure(
                    "inspect existing path ancestry",
                    &current,
                    source,
                ));
            }
        }
    }
}

pub(super) fn metadata_state(path: &Path) -> Result<Option<fs::Metadata>, PathAuthorizationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_failure("inspect destination", path, source)),
    }
}

pub(super) fn require_directory(
    operation: &'static str,
    path: &Path,
) -> Result<(), PathAuthorizationError> {
    let metadata = fs::metadata(path).map_err(|source| io_failure(operation, path, source))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(PathAuthorizationError::ExistingAncestorNotDirectory {
            path: path.to_path_buf(),
        })
    }
}

pub(super) fn canonicalize(
    operation: &'static str,
    path: &Path,
) -> Result<PathBuf, PathAuthorizationError> {
    fs::canonicalize(path).map_err(|source| io_failure(operation, path, source))
}

pub(super) fn io_failure(
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> PathAuthorizationError {
    PathAuthorizationError::Filesystem {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
