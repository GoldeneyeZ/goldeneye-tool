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

mod port;

pub use port::FileArtifactPersistence;

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

mod database;
mod export;
mod git;
mod import;
mod repository;
mod time;

use database::{project_counts, sha256_hex, strip_indexes, unique_sibling, verify_database};
pub use export::export_artifact;
use git::{configure_merge_driver, ensure_gitattributes, git_head, io_error, remove_sidecars};
pub use import::import_artifact;
use repository::{
    artifact_paths, canonical_repository, prepare_artifact_paths, read_bounded, read_metadata,
    sync_parent, verify_regular_file, write_atomic, write_new,
};
use time::iso8601_now;

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
