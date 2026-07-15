use super::{
    ARTIFACT_DIRECTORY, ARTIFACT_FILENAME, ARTIFACT_METADATA, ArtifactError, ArtifactMetadata,
    AtomicWriteFile, File, OpenOptions, Path, PathBuf, Write, fs, io_error,
};

pub(super) fn canonical_repository(path: &Path) -> Result<PathBuf, ArtifactError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| io_error("resolving", path, source))?;
    if !canonical.is_dir() {
        return Err(ArtifactError::UnsafePath(canonical));
    }
    Ok(canonical)
}

pub(super) fn prepare_artifact_paths(
    repository: &Path,
) -> Result<(PathBuf, PathBuf), ArtifactError> {
    let directory = repository.join(ARTIFACT_DIRECTORY);
    if directory.exists() {
        reject_symlink(&directory)?;
    } else {
        fs::create_dir(&directory).map_err(|source| io_error("creating", &directory, source))?;
    }
    let canonical = directory
        .canonicalize()
        .map_err(|source| io_error("resolving", &directory, source))?;
    if !canonical.starts_with(repository) {
        return Err(ArtifactError::UnsafePath(canonical));
    }
    Ok((
        canonical.join(ARTIFACT_FILENAME),
        canonical.join(ARTIFACT_METADATA),
    ))
}

pub(super) fn artifact_paths(repository: &Path) -> Result<(PathBuf, PathBuf), ArtifactError> {
    let repository = canonical_repository(repository)?;
    let directory = repository.join(ARTIFACT_DIRECTORY);
    reject_symlink(&directory)?;
    Ok((
        directory.join(ARTIFACT_FILENAME),
        directory.join(ARTIFACT_METADATA),
    ))
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), ArtifactError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspecting", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(ArtifactError::Symlink(path.to_path_buf()));
    }
    Ok(())
}

pub(super) fn verify_regular_file(path: &Path) -> Result<(), ArtifactError> {
    reject_symlink(path)?;
    if !path.is_file() {
        return Err(ArtifactError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

pub(super) fn read_metadata(path: &Path) -> Result<ArtifactMetadata, ArtifactError> {
    let bytes = read_bounded(path, 128 * 1_024)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ArtifactError> {
    let metadata =
        fs::metadata(path).map_err(|source| io_error("reading metadata for", path, source))?;
    if metadata.len() > limit {
        return Err(ArtifactError::SizeLimit {
            limit,
            actual: metadata.len(),
        });
    }
    fs::read(path).map_err(|source| io_error("reading", path, source))
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
    let mut file = AtomicWriteFile::open(path)
        .map_err(|source| io_error("preparing atomic write for", path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("writing", path, source))?;
    file.commit()
        .map_err(|source| io_error("committing", path, source))
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| io_error("creating", path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("writing", path, source))?;
    file.sync_all()
        .map_err(|source| io_error("syncing", path, source))
}

pub(super) fn sync_parent(path: &Path) -> Result<(), ArtifactError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    match File::open(parent).and_then(|file| file.sync_all()) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(source)
            if matches!(
                source.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(())
        }
        Err(source) => Err(io_error("syncing directory", parent, source)),
    }
}
