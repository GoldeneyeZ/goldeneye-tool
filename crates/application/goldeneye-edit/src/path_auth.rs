//! Filesystem path authorization for durable edit and create operations.

mod lifecycle;
mod validation;

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use goldeneye_domain::{ProjectRelativePath, SyntaxIdentityError};
use thiserror::Error;
use validation::{canonicalize, ensure_project_allowed, parse_relative_path, require_directory};

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
