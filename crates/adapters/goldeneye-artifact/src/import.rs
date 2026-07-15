use super::{
    ARTIFACT_SCHEMA_VERSION, ArtifactError, ArtifactImport, Cursor, MAX_DECOMPRESSED_BYTES, Path,
    Read, artifact_paths, canonical_repository, fs, io_error, read_bounded, read_metadata,
    remove_sidecars, sha256_hex, sync_parent, unique_sibling, verify_database, verify_regular_file,
    write_new,
};

/// Imports and atomically installs a verified `SQLite` artifact.
///
/// The destination database must not be open by another service.
///
/// # Errors
///
/// Returns path, metadata, checksum, size, decompression, integrity, or install errors.
pub fn import_artifact(
    repository: impl AsRef<Path>,
    database_path: impl AsRef<Path>,
) -> Result<ArtifactImport, ArtifactError> {
    let repository = canonical_repository(repository.as_ref())?;
    let (artifact_path, metadata_path) = artifact_paths(&repository)?;
    verify_regular_file(&artifact_path)?;
    verify_regular_file(&metadata_path)?;
    let metadata = read_metadata(&metadata_path)?;
    if metadata.schema_version > ARTIFACT_SCHEMA_VERSION {
        return Err(ArtifactError::SchemaVersion {
            actual: metadata.schema_version,
            maximum: ARTIFACT_SCHEMA_VERSION,
        });
    }
    if metadata.original_size == 0 || metadata.original_size > MAX_DECOMPRESSED_BYTES {
        return Err(ArtifactError::SizeLimit {
            limit: MAX_DECOMPRESSED_BYTES,
            actual: metadata.original_size,
        });
    }
    let compressed = read_bounded(&artifact_path, MAX_DECOMPRESSED_BYTES)?;
    if compressed.len() as u64 != metadata.compressed_size {
        return Err(ArtifactError::MetadataMismatch("compressed_size"));
    }
    if sha256_hex(&compressed) != metadata.sha256.to_ascii_lowercase() {
        return Err(ArtifactError::ChecksumMismatch);
    }

    let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(compressed))
        .map_err(|source| io_error("opening compressed", &artifact_path, source))?;
    let capacity =
        usize::try_from(metadata.original_size).map_err(|_| ArtifactError::SizeLimit {
            limit: u64::try_from(usize::MAX).unwrap_or(u64::MAX),
            actual: metadata.original_size,
        })?;
    let mut decompressed = Vec::with_capacity(capacity);
    decoder
        .by_ref()
        .take(MAX_DECOMPRESSED_BYTES + 1)
        .read_to_end(&mut decompressed)
        .map_err(|source| io_error("decompressing", &artifact_path, source))?;
    if decompressed.len() as u64 > MAX_DECOMPRESSED_BYTES {
        return Err(ArtifactError::SizeLimit {
            limit: MAX_DECOMPRESSED_BYTES,
            actual: decompressed.len() as u64,
        });
    }
    if decompressed.len() as u64 != metadata.original_size {
        return Err(ArtifactError::MetadataMismatch("original_size"));
    }

    let database_path = database_path.as_ref().to_path_buf();
    if let Some(parent) = database_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| io_error("creating", parent, source))?;
    }
    let temp_path = unique_sibling(&database_path, "import.tmp");
    let backup_path = unique_sibling(&database_path, "import.backup");
    write_new(&temp_path, &decompressed)?;
    if let Err(error) = verify_database(&temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    remove_sidecars(&database_path);
    let had_existing = database_path.exists();
    if had_existing {
        fs::rename(&database_path, &backup_path)
            .map_err(|source| io_error("backing up", &database_path, source))?;
    }
    if let Err(source) = fs::rename(&temp_path, &database_path) {
        if had_existing {
            let _ = fs::rename(&backup_path, &database_path);
        }
        let _ = fs::remove_file(&temp_path);
        return Err(io_error("installing", &database_path, source));
    }
    if had_existing {
        let _ = fs::remove_file(&backup_path);
    }
    sync_parent(&database_path)?;
    Ok(ArtifactImport {
        database_path,
        metadata,
    })
}
