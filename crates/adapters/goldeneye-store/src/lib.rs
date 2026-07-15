//! `SQLite` persistence for Goldeneye's tool-neutral code graph.

mod adr;
mod adr_traces_port;
mod crosslink_port;
mod edit_port;
mod git_history_port;
mod index_port;
mod project_administration_port;
mod query_port;
mod repository_factory;
mod schema;
mod semantic_index_port;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use goldeneye_domain::{
    ByteSpan, ContentHash, EdgeDiscriminator, EdgeKind, FileId, FileRecord, Generation, GraphEdge,
    GraphIdentityError, GraphNode, NodeId, NodeLabel, ProjectId, ProjectRecord,
    ProjectRelativePath, QualifiedName, SourcePoint, SourceSpan, SyntaxIdentityError,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Row, Statement, Transaction, TransactionBehavior,
    params,
};
use serde_json::Value;
use thiserror::Error;

pub use adr::{
    ADR_MAX_LENGTH, ADR_MAX_SECTIONS, AdrSection, parse_adr_sections, render_adr_sections,
};
pub use goldeneye_ports::{
    ConnectionSettings, GitCoChangeRecord, GitFileHistoryRecord, GitHistoryOutcome, GraphCounts,
    NodeSignatureRecord, NodeVectorRecord, STORED_VECTOR_DIM, SchemaInfo, SearchHit, StoredVector,
    TokenVectorRecord,
};
pub use repository_factory::SqliteRepositoryFactory;
pub use schema::CURRENT_SCHEMA_VERSION;

const BUSY_TIMEOUT: Duration = Duration::from_secs(10);
pub const MINHASH_SIGNATURE_HEX_LEN: usize = 512;
const NODE_COLUMNS: &str = "project_id, node_id, label, name, qualified_name, file_path, \
    start_byte, end_byte, start_row, start_column, end_row, end_column, generation, properties_json";
const QUALIFIED_NODE_COLUMNS: &str = "nodes.project_id, nodes.node_id, nodes.label, nodes.name, \
    nodes.qualified_name, nodes.file_path, nodes.start_byte, nodes.end_byte, nodes.start_row, \
    nodes.start_column, nodes.end_row, nodes.end_column, nodes.generation, nodes.properties_json";
const EDGE_COLUMNS: &str = "project_id, source_id, target_id, kind, discriminator, generation, \
    properties_json";
const EDIT_JOURNAL_COLUMNS: &str = "operation_id, record_version, operation_kind, project_id, path, \
    original_hash, new_hash, temp_path, backup_path, created_parent_paths_json, phase, created_at, \
    updated_at, last_error";
const UPSERT_FILE_SQL: &str = "INSERT INTO files(\
    project_id, path, content_hash, generation, modified_ns, byte_len\
  ) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
  ON CONFLICT(project_id, path) DO UPDATE SET \
    content_hash = excluded.content_hash, generation = excluded.generation, \
    modified_ns = excluded.modified_ns, byte_len = excluded.byte_len";
const INSERT_NODE_SQL: &str = "INSERT INTO nodes(\
    project_id, node_id, label, name, qualified_name, file_path, \
    start_byte, end_byte, start_row, start_column, end_row, end_column, \
    generation, properties_json\
  ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)";
const INSERT_EDGE_SQL: &str = "INSERT INTO edges(\
    project_id, source_id, target_id, kind, discriminator, generation, properties_json\
  ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite store error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON store error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database does not exist: {0}")]
    DatabaseNotFound(PathBuf),
    #[error("database schema version {actual} is newer than supported version {supported}")]
    SchemaTooNew { actual: u32, supported: u32 },
    #[error("project not found: {0:?}")]
    ProjectNotFound(ProjectId),
    #[error("file not found: {0:?}")]
    FileNotFound(FileId),
    #[error("generation overflow for project: {0:?}")]
    GenerationOverflow(ProjectId),
    #[error("generation mismatch: expected {expected:?}, got {actual:?}")]
    GenerationMismatch {
        expected: Generation,
        actual: Generation,
    },
    #[error("project mismatch: expected {expected:?}, got {actual:?}")]
    ProjectMismatch {
        expected: ProjectId,
        actual: ProjectId,
    },
    #[error("node belongs to file {actual:?}, expected {expected:?}")]
    FileMismatch {
        expected: ProjectRelativePath,
        actual: Option<ProjectRelativePath>,
    },
    #[error("graph references missing node: {node_id:?}")]
    MissingNode { node_id: NodeId },
    #[error("duplicate node ID in replacement: {0:?}")]
    DuplicateNodeId(NodeId),
    #[error("duplicate file path in replacement: {0:?}")]
    DuplicateFilePath(ProjectRelativePath),
    #[error("duplicate qualified name in replacement: {0:?}")]
    DuplicateQualifiedName(QualifiedName),
    #[error("duplicate edge identity in replacement")]
    DuplicateEdge,
    #[error("stored {field} is corrupt: {reason}")]
    CorruptData { field: &'static str, reason: String },
    #[error("numeric value does not fit SQLite INTEGER: {field}={value}")]
    NumericOverflow { field: &'static str, value: u64 },
    #[error("edit operation ID must be non-empty and contain no NUL bytes")]
    InvalidEditOperationId,
    #[error("invalid edit journal record: {reason}")]
    InvalidEditJournalRecord { reason: &'static str },
    #[error("edit operation not found: {0:?}")]
    EditOperationNotFound(EditOperationId),
    #[error("an incomplete edit already owns {project_id:?}:{path:?}")]
    EditTargetBusy {
        project_id: ProjectId,
        path: ProjectRelativePath,
    },
    #[error("invalid edit phase transition from {from:?} to {to:?}")]
    InvalidEditPhaseTransition { from: EditPhase, to: EditPhase },
    #[error("stale edit phase: expected {expected:?}, actual {actual:?}")]
    StaleEditPhase {
        expected: EditPhase,
        actual: EditPhase,
    },
    #[error("no ADR found for project: {0:?}")]
    AdrNotFound(ProjectId),
    #[error("merged ADR exceeds {limit} chars ({actual} chars)")]
    AdrTooLarge { limit: usize, actual: usize },
    #[error("invalid runtime trace: {reason}")]
    InvalidRuntimeTrace { reason: &'static str },
    #[error("invalid Git history record: {reason}")]
    InvalidGitHistory { reason: &'static str },
    #[error("invalid semantic index record: {reason}")]
    InvalidSemanticRecord { reason: String },
    #[error("invalid cross-project edge: {reason}")]
    InvalidCrossProjectEdge { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacementOutcome {
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectReplacementOutcome {
    pub generation: Generation,
    pub files: usize,
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileOutcome {
    pub removed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdrRecord {
    pub project: ProjectId,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTrace {
    pub caller: String,
    pub callee: String,
    pub count: u64,
}

impl RuntimeTrace {
    /// Creates one validated runtime edge observation.
    ///
    /// # Errors
    ///
    /// Returns an error for empty endpoints, embedded NUL bytes, or a zero count.
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        count: u64,
    ) -> Result<Self, StoreError> {
        let trace = Self {
            caller: source.into(),
            callee: target.into(),
            count,
        };
        validate_runtime_trace(&trace)?;
        Ok(trace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTraceRecord {
    pub project: ProjectId,
    pub caller: String,
    pub callee: String,
    pub count: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticIndexOutcome {
    pub node_vectors: usize,
    pub token_vectors: usize,
    pub node_signatures: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditOperationId(String);

impl EditOperationId {
    /// Creates a stable edit operation identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidEditOperationId`] for an empty or NUL-containing value.
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if value.is_empty() || value.contains('\0') {
            return Err(StoreError::InvalidEditOperationId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOperationKind {
    Create,
    Update,
    Delete,
}

impl EditOperationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    fn from_stored(value: &str) -> Result<Self, StoreError> {
        match value {
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            _ => Err(StoreError::CorruptData {
                field: "edit operation kind",
                reason: format!("unknown value {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPhase {
    Prepared,
    BackupReady,
    Replaced,
    Indexed,
    Committed,
    RolledBack,
}

impl EditPhase {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Committed | Self::RolledBack)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::BackupReady => "backup_ready",
            Self::Replaced => "replaced",
            Self::Indexed => "indexed",
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
        }
    }

    const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Prepared, Self::BackupReady)
                | (Self::BackupReady, Self::Replaced)
                | (Self::Replaced, Self::Indexed)
                | (Self::Indexed, Self::Committed)
        ) || (!self.is_terminal() && matches!(next, Self::RolledBack))
    }

    fn from_stored(value: &str) -> Result<Self, StoreError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "backup_ready" => Ok(Self::BackupReady),
            "replaced" => Ok(Self::Replaced),
            "indexed" => Ok(Self::Indexed),
            "committed" => Ok(Self::Committed),
            "rolled_back" => Ok(Self::RolledBack),
            _ => Err(StoreError::CorruptData {
                field: "edit phase",
                reason: format!("unknown value {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEditJournalRecord {
    pub operation_id: EditOperationId,
    pub operation_kind: EditOperationKind,
    pub project_id: ProjectId,
    pub path: ProjectRelativePath,
    pub original_hash: Option<ContentHash>,
    pub new_hash: Option<ContentHash>,
    pub temp_path: Option<ProjectRelativePath>,
    pub backup_path: Option<ProjectRelativePath>,
    pub created_parent_paths: Vec<ProjectRelativePath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditJournalRecord {
    pub operation_id: EditOperationId,
    pub record_version: u32,
    pub operation_kind: EditOperationKind,
    pub project_id: ProjectId,
    pub path: ProjectRelativePath,
    pub original_hash: Option<ContentHash>,
    pub new_hash: Option<ContentHash>,
    pub temp_path: Option<ProjectRelativePath>,
    pub backup_path: Option<ProjectRelativePath>,
    pub created_parent_paths: Vec<ProjectRelativePath>,
    pub phase: EditPhase,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
}

pub struct Store {
    connection: Connection,
}

pub struct QueryStore {
    connection: Connection,
}

mod adr_traces;
mod codec;
mod cross_project;
mod git_history;
mod graph;
mod graph_support;
mod journal;
mod project;
mod read;
mod read_helpers;
mod semantic;

use adr_traces::{get_adr, list_runtime_traces, validate_runtime_trace};
use codec::{
    corrupt_domain, corrupt_graph, corrupt_syntax, source_span_from_raw, sql_span, sqlite_integer,
    sqlite_u64,
};
use git_history::{coupled_files, list_git_cochanges, list_git_file_history};
use graph_support::{
    ensure_generation, ensure_node_exists, ensure_project_exists, insert_edge, insert_edge_with,
    insert_node, insert_node_with, project_file_paths, project_generation, upsert_file_in,
    upsert_file_with, validate_project_replacement, validate_replacement,
};
use journal::{get_edit_operation, list_incomplete_edit_operations};
use project::connection_settings;
use read_helpers::{
    count_search_nodes, counts, edges_from, edges_to, get_file, get_node, get_project, list_edges,
    list_files, list_nodes, list_projects, node_by_qualified_name, nodes_for_file,
    search_nodes_page,
};
use semantic::{
    get_node_signature, get_node_vector, get_token_vector, list_node_signatures, list_node_vectors,
};
