use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{
    AuthorizedPath, CreatedDirectories, PathAuthorizationError, PathIntent, RevalidatedPath,
    validation::{
        canonicalize, ensure_project_allowed, io_failure, metadata_state,
        require_confined_directory, validate_existing_ancestry,
    },
};

impl AuthorizedPath {
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
        let current_root = self.revalidate_project_root()?;
        let destination_state = metadata_state(&self.destination)?;
        if self.intent == PathIntent::Create && destination_state.is_some() {
            return Err(PathAuthorizationError::DestinationExists {
                path: self.destination.clone(),
            });
        }
        validate_existing_ancestry(&current_root, &self.destination)?;
        self.validate_destination_state(destination_state)?;
        Ok(RevalidatedPath {
            destination: self.destination.clone(),
        })
    }

    fn revalidate_project_root(&self) -> Result<PathBuf, PathAuthorizationError> {
        let current_root = canonicalize("revalidate project root", &self.project_root)?;
        if current_root != self.project_root {
            return Err(PathAuthorizationError::ProjectRootChanged {
                expected: self.project_root.clone(),
                actual: current_root,
            });
        }
        ensure_project_allowed(&current_root, &self.allowed_roots)?;
        Ok(current_root)
    }

    fn validate_destination_state(
        &self,
        destination_state: Option<fs::Metadata>,
    ) -> Result<(), PathAuthorizationError> {
        match (self.intent, destination_state) {
            (PathIntent::Update, None) => Err(PathAuthorizationError::DestinationMissing {
                path: self.destination.clone(),
            }),
            (PathIntent::Update, Some(_)) => {
                let metadata = fs::metadata(&self.destination).map_err(|source| {
                    io_failure("inspect update destination", &self.destination, source)
                })?;
                if !metadata.is_file() {
                    return Err(PathAuthorizationError::DestinationNotFile {
                        path: self.destination.clone(),
                    });
                }
                Ok(())
            }
            _ => Ok(()),
        }
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
        let mut created = Vec::new();
        let result = self.create_missing_parents(&mut created);
        if let Err(error) = result {
            let _ = rollback_empty_paths(&self.project_root, &created);
            return Err(error);
        }
        Ok(CreatedDirectories {
            project_root: self.project_root.clone(),
            paths: created,
        })
    }

    fn create_missing_parents(
        &self,
        created: &mut Vec<PathBuf>,
    ) -> Result<(), PathAuthorizationError> {
        let parent_segments = self.relative_path.as_str().split('/');
        let parent_count = parent_segments.clone().count().saturating_sub(1);
        let mut current = self.project_root.clone();
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
    }
}

impl CreatedDirectories {
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
