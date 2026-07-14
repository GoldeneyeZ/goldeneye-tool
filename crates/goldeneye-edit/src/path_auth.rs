//! Filesystem path authorization for durable edit and create operations.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use goldeneye_domain::{ProjectRelativePath, SyntaxIdentityError};
use thiserror::Error;

const RESERVED_SEGMENTS: [&str; 2] = [".goldeneye", ".codebase-memory"];

/// Declares whether an authorized destination must be new or already exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathIntent {
    Create,
    Update,
}

/// Configured filesystem boundary for project mutations.
#[derive(Debug, Clone)]
pub struct PathAuthorizer {
    allowed_roots: Arc<[PathBuf]>,
}

impl PathAuthorizer {
    /// Canonicalizes and validates the roots beneath which projects may be edited.
    ///
    /// # Errors
    ///
    /// Returns an error when no roots are configured, a root cannot be
    /// canonicalized, or a root is not a directory.
    pub fn new<I, P>(allowed_roots: I) -> Result<Self, PathAuthorizationError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut canonical_roots = Vec::new();
        for root in allowed_roots {
            let requested = root.as_ref();
            let canonical = canonicalize("canonicalize allowed root", requested)?;
            require_directory("inspect allowed root", &canonical)?;
            if !canonical_roots.contains(&canonical) {
                canonical_roots.push(canonical);
            }
        }
        if canonical_roots.is_empty() {
            return Err(PathAuthorizationError::NoAllowedRoots);
        }
        Ok(Self {
            allowed_roots: Arc::from(canonical_roots),
        })
    }

    /// Authorizes a normalized project-relative destination for one intent.
    ///
    /// The returned authorization retains the canonical boundary information
    /// needed to revalidate the destination immediately before a mutation.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid or reserved relative paths, projects outside
    /// the configured roots, containment escapes, and intent/state mismatches.
    pub fn authorize(
        &self,
        project_root: impl AsRef<Path>,
        relative_path: &str,
        intent: PathIntent,
    ) -> Result<AuthorizedPath, PathAuthorizationError> {
        let project_root = canonicalize("canonicalize project root", project_root.as_ref())?;
        require_directory("inspect project root", &project_root)?;
        ensure_project_allowed(&project_root, &self.allowed_roots)?;

        let relative_path = parse_relative_path(relative_path)?;
        let destination = relative_path
            .as_str()
            .split('/')
            .fold(project_root.clone(), |path, segment| path.join(segment));
        let authorized = AuthorizedPath {
            allowed_roots: Arc::clone(&self.allowed_roots),
            project_root,
            relative_path,
            destination,
            intent,
        };
        authorized.revalidate()?;
        Ok(authorized)
    }
}

/// A destination whose lexical path and current filesystem ancestry passed authorization.
#[derive(Debug, Clone)]
pub struct AuthorizedPath {
    allowed_roots: Arc<[PathBuf]>,
    project_root: PathBuf,
    relative_path: ProjectRelativePath,
    destination: PathBuf,
    intent: PathIntent,
}

impl AuthorizedPath {
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    #[must_use]
    pub fn relative_path(&self) -> &ProjectRelativePath {
        &self.relative_path
    }

    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    #[must_use]
    pub const fn intent(&self) -> PathIntent {
        self.intent
    }

    /// Re-checks the canonical project and nearest existing destination ancestry.
    ///
    /// Call this immediately before the filesystem mutation. The check follows
    /// symlinks, junctions, and other reparse-point ancestry through
    /// [`fs::canonicalize`] and rejects any resolution outside the project.
    ///
    /// # Errors
    ///
    /// Returns an error when the project boundary changed, ancestry escapes,
    /// filesystem inspection fails, or the destination no longer matches its
    /// create/update intent.
    pub fn revalidate(&self) -> Result<RevalidatedPath, PathAuthorizationError> {
        let current_root = canonicalize("revalidate project root", &self.project_root)?;
        if current_root != self.project_root {
            return Err(PathAuthorizationError::ProjectRootChanged {
                expected: self.project_root.clone(),
                actual: current_root,
            });
        }
        ensure_project_allowed(&current_root, &self.allowed_roots)?;

        let destination_state = metadata_state(&self.destination)?;
        if self.intent == PathIntent::Create && destination_state.is_some() {
            return Err(PathAuthorizationError::DestinationExists {
                path: self.destination.clone(),
            });
        }

        validate_existing_ancestry(&current_root, &self.destination)?;
        match (self.intent, destination_state) {
            (PathIntent::Update, None) => {
                return Err(PathAuthorizationError::DestinationMissing {
                    path: self.destination.clone(),
                });
            }
            (PathIntent::Update, Some(_)) => {
                let metadata = fs::metadata(&self.destination).map_err(|source| {
                    io_failure("inspect update destination", &self.destination, source)
                })?;
                if !metadata.is_file() {
                    return Err(PathAuthorizationError::DestinationNotFile {
                        path: self.destination.clone(),
                    });
                }
            }
            _ => {}
        }

        Ok(RevalidatedPath {
            destination: self.destination.clone(),
        })
    }

    /// Creates missing destination parents one component at a time.
    ///
    /// Every existing or newly created component is canonicalized beneath the
    /// project. The returned report can remove the same directories later when
    /// they are still empty.
    ///
    /// # Errors
    ///
    /// Returns an error for update intents, containment changes, non-directory
    /// ancestors, or filesystem failures. Directories created before a failure
    /// are rolled back when still empty.
    pub fn create_parent_directories(&self) -> Result<CreatedDirectories, PathAuthorizationError> {
        if self.intent != PathIntent::Create {
            return Err(PathAuthorizationError::ParentCreationRequiresCreate);
        }
        self.revalidate()?;

        let parent_segments = self.relative_path.as_str().split('/');
        let parent_count = parent_segments.clone().count().saturating_sub(1);
        let mut current = self.project_root.clone();
        let mut created = Vec::new();
        let result = (|| {
            for segment in parent_segments.take(parent_count) {
                current.push(segment);
                if metadata_state(&current)?.is_none() {
                    validate_existing_ancestry(&self.project_root, &current)?;
                    match fs::create_dir(&current) {
                        Ok(()) => created.push(current.clone()),
                        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(source) => {
                            return Err(io_failure("create parent directory", &current, source));
                        }
                    }
                }
                require_confined_directory(&self.project_root, &current)?;
            }
            self.revalidate()?;
            Ok(())
        })();

        if let Err(error) = result {
            let _ = rollback_empty_paths(&self.project_root, &created);
            return Err(error);
        }
        Ok(CreatedDirectories {
            project_root: self.project_root.clone(),
            paths: created,
        })
    }
}

/// A destination produced by the latest containment and intent check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidatedPath {
    destination: PathBuf,
}

impl RevalidatedPath {
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.destination
    }
}

impl AsRef<Path> for RevalidatedPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

/// Directories created while preparing one authorized create destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedDirectories {
    project_root: PathBuf,
    paths: Vec<PathBuf>,
}

impl CreatedDirectories {
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Removes reported directories in reverse order when they remain empty.
    ///
    /// Non-empty and already removed directories are retained without error.
    ///
    /// # Errors
    ///
    /// Returns an error when a reported path no longer resolves beneath the
    /// original project or a filesystem removal fails for another reason.
    pub fn rollback_empty(&self) -> Result<(), PathAuthorizationError> {
        rollback_empty_paths(&self.project_root, &self.paths)
    }
}

#[derive(Debug, Error)]
pub enum PathAuthorizationError {
    #[error("at least one allowed root must be configured")]
    NoAllowedRoots,
    #[error("invalid project-relative path {path:?}: {source}")]
    InvalidRelativePath {
        path: String,
        #[source]
        source: SyntaxIdentityError,
    },
    #[error("project-relative path {path:?} contains unsupported platform component {segment:?}")]
    UnsupportedPlatformComponent { path: String, segment: String },
    #[error("project-relative path {path:?} enters reserved metadata segment {segment:?}")]
    ReservedMetadata { path: String, segment: String },
    #[error("project root is outside configured allowed roots: {path}", path = path.display())]
    ProjectOutsideAllowedRoots { path: PathBuf },
    #[error(
        "project root changed since authorization: expected {expected}, resolved {actual}",
        expected = expected.display(),
        actual = actual.display()
    )]
    ProjectRootChanged { expected: PathBuf, actual: PathBuf },
    #[error(
        "path escapes project: {path} resolves through {resolved}",
        path = path.display(),
        resolved = resolved.display()
    )]
    PathEscapesProject { path: PathBuf, resolved: PathBuf },
    #[error("create destination already exists: {path}", path = path.display())]
    DestinationExists { path: PathBuf },
    #[error("update destination does not exist: {path}", path = path.display())]
    DestinationMissing { path: PathBuf },
    #[error("update destination is not a file: {path}", path = path.display())]
    DestinationNotFile { path: PathBuf },
    #[error("existing path component is not a directory: {path}", path = path.display())]
    ExistingAncestorNotDirectory { path: PathBuf },
    #[error("parent directory creation is only valid for create destinations")]
    ParentCreationRequiresCreate,
    #[error("{operation} failed for {path}: {source}", path = path.display())]
    Filesystem {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn parse_relative_path(value: &str) -> Result<ProjectRelativePath, PathAuthorizationError> {
    let relative_path = ProjectRelativePath::new(value.to_owned()).map_err(|source| {
        PathAuthorizationError::InvalidRelativePath {
            path: value.to_owned(),
            source,
        }
    })?;
    for segment in value.split('/') {
        if segment.contains(':') {
            return Err(PathAuthorizationError::UnsupportedPlatformComponent {
                path: value.to_owned(),
                segment: segment.to_owned(),
            });
        }
        if RESERVED_SEGMENTS
            .iter()
            .any(|reserved| segment.eq_ignore_ascii_case(reserved))
        {
            return Err(PathAuthorizationError::ReservedMetadata {
                path: value.to_owned(),
                segment: segment.to_owned(),
            });
        }
    }
    Ok(relative_path)
}

fn ensure_project_allowed(
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

fn validate_existing_ancestry(
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

fn require_confined_directory(
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

fn metadata_state(path: &Path) -> Result<Option<fs::Metadata>, PathAuthorizationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_failure("inspect destination", path, source)),
    }
}

fn require_directory(operation: &'static str, path: &Path) -> Result<(), PathAuthorizationError> {
    let metadata = fs::metadata(path).map_err(|source| io_failure(operation, path, source))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(PathAuthorizationError::ExistingAncestorNotDirectory {
            path: path.to_path_buf(),
        })
    }
}

fn rollback_empty_paths(
    project_root: &Path,
    paths: &[PathBuf],
) -> Result<(), PathAuthorizationError> {
    for path in paths.iter().rev() {
        if path == project_root || !path.starts_with(project_root) {
            return Err(PathAuthorizationError::PathEscapesProject {
                path: path.clone(),
                resolved: path.clone(),
            });
        }
        if metadata_state(path)?.is_none() {
            continue;
        }
        require_confined_directory(project_root, path)?;
        match fs::remove_dir(path) {
            Ok(()) => {}
            Err(source)
                if matches!(
                    source.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(source) => return Err(io_failure("remove empty parent directory", path, source)),
        }
    }
    Ok(())
}

fn canonicalize(operation: &'static str, path: &Path) -> Result<PathBuf, PathAuthorizationError> {
    fs::canonicalize(path).map_err(|source| io_failure(operation, path, source))
}

fn io_failure(operation: &'static str, path: &Path, source: io::Error) -> PathAuthorizationError {
    PathAuthorizationError::Filesystem {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
