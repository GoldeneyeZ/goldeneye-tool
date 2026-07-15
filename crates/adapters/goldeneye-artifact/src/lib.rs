//! Safe compressed knowledge-graph artifacts.

use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_write_file::AtomicWriteFile;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const ARTIFACT_SCHEMA_VERSION: u32 = 2;
pub const ARTIFACT_DIRECTORY: &str = ".codebase-memory";
pub const ARTIFACT_FILENAME: &str = "graph.db.zst";
pub const ARTIFACT_METADATA: &str = "artifact.json";
pub const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1_024 * 1_024;

const EXPLICIT_INDEXES: &[&str] = &[
    "files_generation_idx",
    "nodes_file_idx",
    "nodes_label_idx",
    "edges_source_idx",
    "edges_target_idx",
    "edit_journal_project_path_idx",
    "edit_journal_incomplete_idx",
    "edit_journal_active_target_idx",
    "runtime_traces_project_count_idx",
    "git_file_history_recent_idx",
    "git_cochanges_file_b_idx",
    "git_cochanges_score_idx",
    "node_vectors_project_idx",
    "node_signatures_project_idx",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactQuality {
    Fast,
    Best,
}

impl ArtifactQuality {
    const fn compression_level(self) -> i32 {
        match self {
            Self::Fast => 3,
            Self::Best => 9,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub schema_version: u32,
    pub commit: String,
    pub indexed_at: String,
    pub project: String,
    pub nodes: u64,
    pub edges: u64,
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_level: i32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactExport {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub metadata: ArtifactMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactImport {
    pub database_path: PathBuf,
    pub metadata: ArtifactMetadata,
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("artifact path is outside or aliases repository root: {0}")]
    UnsafePath(PathBuf),
    #[error("artifact component must not be a symlink: {0}")]
    Symlink(PathBuf),
    #[error("artifact size {actual} exceeds limit {limit}")]
    SizeLimit { limit: u64, actual: u64 },
    #[error("unsupported artifact schema {actual}; maximum is {maximum}")]
    SchemaVersion { actual: u32, maximum: u32 },
    #[error("artifact metadata does not match payload: {0}")]
    MetadataMismatch(&'static str),
    #[error("artifact checksum mismatch")]
    ChecksumMismatch,
    #[error("SQLite integrity check failed: {0}")]
    Integrity(String),
    #[error("I/O error while {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SQLite artifact error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("artifact metadata JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn artifact_exists(repository: impl AsRef<Path>) -> bool {
    artifact_paths(repository.as_ref())
        .is_ok_and(|(artifact, metadata)| artifact.is_file() && metadata.is_file())
}

/// Returns the commit recorded in the repository artifact metadata, if present.
///
/// # Errors
///
/// Returns path, I/O, or metadata decoding errors.
pub fn artifact_commit(repository: impl AsRef<Path>) -> Result<Option<String>, ArtifactError> {
    let (_, metadata_path) = artifact_paths(repository.as_ref())?;
    let metadata = read_metadata(&metadata_path)?;
    Ok((!metadata.commit.is_empty()).then_some(metadata.commit))
}

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

fn canonical_repository(path: &Path) -> Result<PathBuf, ArtifactError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| io_error("resolving", path, source))?;
    if !canonical.is_dir() {
        return Err(ArtifactError::UnsafePath(canonical));
    }
    Ok(canonical)
}

fn prepare_artifact_paths(repository: &Path) -> Result<(PathBuf, PathBuf), ArtifactError> {
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

fn artifact_paths(repository: &Path) -> Result<(PathBuf, PathBuf), ArtifactError> {
    let repository = canonical_repository(repository)?;
    let directory = repository.join(ARTIFACT_DIRECTORY);
    reject_symlink(&directory)?;
    Ok((
        directory.join(ARTIFACT_FILENAME),
        directory.join(ARTIFACT_METADATA),
    ))
}

fn reject_symlink(path: &Path) -> Result<(), ArtifactError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspecting", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(ArtifactError::Symlink(path.to_path_buf()));
    }
    Ok(())
}

fn verify_regular_file(path: &Path) -> Result<(), ArtifactError> {
    reject_symlink(path)?;
    if !path.is_file() {
        return Err(ArtifactError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn read_metadata(path: &Path) -> Result<ArtifactMetadata, ArtifactError> {
    let bytes = read_bounded(path, 128 * 1_024)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ArtifactError> {
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
    let mut file = AtomicWriteFile::open(path)
        .map_err(|source| io_error("preparing atomic write for", path, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("writing", path, source))?;
    file.commit()
        .map_err(|source| io_error("committing", path, source))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
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

fn sync_parent(path: &Path) -> Result<(), ArtifactError> {
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

fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    path.with_file_name(format!(".{name}.{suffix}.{}.{}", std::process::id(), nonce))
}

fn strip_indexes(path: &Path) -> Result<(), ArtifactError> {
    let connection = Connection::open(path)?;
    for index in EXPLICIT_INDEXES {
        connection.execute_batch(&format!("DROP INDEX IF EXISTS \"{index}\";"))?;
    }
    connection.execute_batch("VACUUM;")?;
    Ok(())
}

fn verify_database(path: &Path) -> Result<(), ArtifactError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(ArtifactError::Integrity(result));
    }
    let projects = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='projects'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let projects = nonnegative_count(projects, "projects table")?;
    if projects != 1 {
        return Err(ArtifactError::Integrity("missing projects table".into()));
    }
    Ok(())
}

fn project_counts(path: &Path, project: &str) -> Result<(u64, u64), ArtifactError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let nodes = connection.query_row(
        "SELECT count(*) FROM nodes WHERE project_id = ?1",
        [project],
        |row| row.get::<_, i64>(0),
    )?;
    let edges = connection.query_row(
        "SELECT count(*) FROM edges WHERE project_id = ?1",
        [project],
        |row| row.get::<_, i64>(0),
    )?;
    Ok((
        nonnegative_count(nodes, "node")?,
        nonnegative_count(edges, "edge")?,
    ))
}

fn nonnegative_count(value: i64, subject: &str) -> Result<u64, ArtifactError> {
    u64::try_from(value)
        .map_err(|_| ArtifactError::Integrity(format!("negative {subject} count: {value}")))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn git_head(repository: &Path) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "HEAD"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .unwrap_or_default()
}

fn configure_merge_driver(repository: &Path) {
    let _ = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["config", "merge.ours.driver", "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn ensure_gitattributes(repository: &Path) -> Result<(), ArtifactError> {
    let path = repository.join(ARTIFACT_DIRECTORY).join(".gitattributes");
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(
                b"# Auto-generated by Goldeneye\n# Prevent merge conflicts on compressed artifact\ngraph.db.zst binary merge=ours\n",
            )
            .map_err(|source| io_error("writing", &path, source))?;
            file.sync_all()
                .map_err(|source| io_error("syncing", &path, source))?;
            Ok(())
        }
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(io_error("creating", &path, source)),
    }
}

fn remove_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        let _ = fs::remove_file(sidecar);
    }
}

fn io_error(operation: &'static str, path: &Path, source: std::io::Error) -> ArtifactError {
    ArtifactError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn iso8601_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{ArtifactQuality, export_artifact, import_artifact};

    #[test]
    fn artifact_round_trip_preserves_verified_sqlite_snapshot() {
        let temp = TempDir::new().expect("temp");
        let source = temp.path().join("source.db");
        let destination = temp.path().join("destination.db");
        let connection = Connection::open(&source).expect("source");
        connection
            .execute_batch(
                "CREATE TABLE projects(project_id TEXT PRIMARY KEY);\n\
                 CREATE TABLE nodes(project_id TEXT);\n\
                 CREATE TABLE edges(project_id TEXT);\n\
                 INSERT INTO projects VALUES ('fixture');\n\
                 INSERT INTO nodes VALUES ('fixture');\n\
                 INSERT INTO edges VALUES ('fixture');",
            )
            .expect("schema");
        drop(connection);

        let exported = export_artifact(&source, temp.path(), "fixture", ArtifactQuality::Fast)
            .expect("export");
        assert_eq!(exported.metadata.nodes, 1);
        assert_eq!(exported.metadata.edges, 1);
        let imported = import_artifact(temp.path(), &destination).expect("import");
        assert_eq!(imported.metadata.sha256, exported.metadata.sha256);
        let destination = Connection::open(destination).expect("destination");
        assert_eq!(
            destination
                .query_row("SELECT count(*) FROM projects", [], |row| row
                    .get::<_, i64>(0))
                .expect("count"),
            1
        );
    }
}
