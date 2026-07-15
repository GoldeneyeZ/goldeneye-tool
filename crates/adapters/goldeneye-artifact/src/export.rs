use super::{
    ARTIFACT_SCHEMA_VERSION, ArtifactError, ArtifactExport, ArtifactMetadata, ArtifactQuality,
    Connection, Cursor, MAX_DECOMPRESSED_BYTES, OpenFlags, Path, canonical_repository,
    configure_merge_driver, ensure_gitattributes, fs, git_head, io_error, iso8601_now, params,
    prepare_artifact_paths, project_counts, read_bounded, remove_sidecars, sha256_hex,
    strip_indexes, unique_sibling, verify_database, write_atomic,
};

/// Exports a transactionally consistent `SQLite` snapshot.
///
/// # Errors
///
/// Returns path, I/O, `SQLite`, size, compression, or metadata errors.
pub fn export_artifact(
    database_path: impl AsRef<Path>,
    repository: impl AsRef<Path>,
    project: &str,
    quality: ArtifactQuality,
) -> Result<ArtifactExport, ArtifactError> {
    let database_path = database_path.as_ref();
    let repository = canonical_repository(repository.as_ref())?;
    let (artifact_path, metadata_path) = prepare_artifact_paths(&repository)?;
    let snapshot_path = unique_sibling(&artifact_path, "snapshot.db");

    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute(
        "VACUUM INTO ?1",
        params![snapshot_path.to_string_lossy().as_ref()],
    )?;
    drop(connection);

    let result = (|| {
        if quality == ArtifactQuality::Best {
            strip_indexes(&snapshot_path)?;
        }
        verify_database(&snapshot_path)?;
        let original = read_bounded(&snapshot_path, MAX_DECOMPRESSED_BYTES)?;
        let original_size = original.len() as u64;
        let compressed =
            zstd::stream::encode_all(Cursor::new(original), quality.compression_level())
                .map_err(|source| io_error("compressing", &snapshot_path, source))?;
        if compressed.len() as u64 > MAX_DECOMPRESSED_BYTES {
            return Err(ArtifactError::SizeLimit {
                limit: MAX_DECOMPRESSED_BYTES,
                actual: compressed.len() as u64,
            });
        }
        let sha256 = sha256_hex(&compressed);
        let (nodes, edges) = project_counts(database_path, project)?;
        let metadata = ArtifactMetadata {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            commit: git_head(&repository),
            indexed_at: iso8601_now(),
            project: project.to_owned(),
            nodes,
            edges,
            original_size,
            compressed_size: compressed.len() as u64,
            compression_level: quality.compression_level(),
            sha256,
        };
        write_atomic(&artifact_path, &compressed)?;
        let encoded = serde_json::to_vec_pretty(&metadata)?;
        if let Err(error) = write_atomic(&metadata_path, &encoded) {
            let _ = fs::remove_file(&artifact_path);
            return Err(error);
        }
        ensure_gitattributes(&repository)?;
        configure_merge_driver(&repository);
        Ok(ArtifactExport {
            artifact_path,
            metadata_path,
            metadata,
        })
    })();
    let _ = fs::remove_file(&snapshot_path);
    remove_sidecars(&snapshot_path);
    result
}
