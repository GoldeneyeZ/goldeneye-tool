//! `SQLite` persistence for Goldeneye's tool-neutral code graph.

mod adr;
mod schema;

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
    Connection, OpenFlags, OptionalExtension, Row, Transaction, TransactionBehavior, params,
};
use serde_json::Value;
use thiserror::Error;

pub use adr::{
    ADR_MAX_LENGTH, ADR_MAX_SECTIONS, AdrSection, parse_adr_sections, render_adr_sections,
};
pub use schema::CURRENT_SCHEMA_VERSION;

const BUSY_TIMEOUT: Duration = Duration::from_secs(10);
pub const STORED_VECTOR_DIM: usize = 768;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub version: u32,
    pub tables: BTreeSet<String>,
    pub indexes: BTreeSet<String>,
    pub fts5_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSettings {
    pub foreign_keys: bool,
    pub journal_mode: String,
    pub synchronous: i64,
    pub busy_timeout_ms: u64,
    pub query_only: bool,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphCounts {
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileHistoryRecord {
    pub path: ProjectRelativePath,
    pub change_count: u64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GitCoChangeRecord {
    pub file_a: ProjectRelativePath,
    pub file_b: ProjectRelativePath,
    pub co_changes: u64,
    pub coupling_score: f64,
    pub last_co_change: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitHistoryOutcome {
    pub files: usize,
    pub couplings: usize,
    pub enriched_files: usize,
    pub enriched_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVector([i8; STORED_VECTOR_DIM]);

impl StoredVector {
    #[must_use]
    pub const fn from_array(values: [i8; STORED_VECTOR_DIM]) -> Self {
        Self(values)
    }

    #[must_use]
    pub const fn values(&self) -> &[i8; STORED_VECTOR_DIM] {
        &self.0
    }

    fn to_blob(&self) -> Vec<u8> {
        self.0.iter().map(|value| value.to_ne_bytes()[0]).collect()
    }

    fn from_blob(blob: Vec<u8>, field: &'static str) -> Result<Self, StoreError> {
        let bytes: [u8; STORED_VECTOR_DIM] =
            blob.try_into()
                .map_err(|value: Vec<u8>| StoreError::CorruptData {
                    field,
                    reason: format!("expected {STORED_VECTOR_DIM} bytes, found {}", value.len()),
                })?;
        Ok(Self(bytes.map(|value| i8::from_ne_bytes([value]))))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVectorRecord {
    pub node_id: NodeId,
    pub vector: StoredVector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenVectorRecord {
    pub token: String,
    pub vector: StoredVector,
    /// Inverse-document frequency multiplied by 1,000, matching upstream storage.
    pub idf_milli: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSignatureRecord {
    pub node_id: NodeId,
    pub minhash_hex: String,
    pub ast_profile: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticIndexOutcome {
    pub node_vectors: usize,
    pub token_vectors: usize,
    pub node_signatures: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: GraphNode,
    pub rank: f64,
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

impl Store {
    /// Opens or creates a durable `SQLite` store and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Returns a typed store error when opening, configuring, or migrating fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        configure_writable(&connection, false)?;
        schema::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Opens an isolated in-memory store.
    ///
    /// # Errors
    ///
    /// Returns a typed store error when configuring or migrating fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory()?;
        configure_writable(&connection, true)?;
        schema::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Opens an existing database with `SQLite` read-only and query-only guards.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::DatabaseNotFound`] without creating a file when absent.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<QueryStore, StoreError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(StoreError::DatabaseNotFound(path.to_path_buf()));
        }
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        configure_read_only(&connection)?;
        Ok(QueryStore { connection })
    }

    /// Registers a project or updates its root path without rewinding generation.
    ///
    /// # Errors
    ///
    /// Returns a store error when the write fails.
    pub fn register_project(&mut self, project: &ProjectRecord) -> Result<(), StoreError> {
        let generation = sqlite_integer("project generation", project.generation.value())?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO projects(id, root_path, current_generation) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![project.id.as_str(), project.root_path, generation],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Deletes a project and all dependent files and graph records.
    ///
    /// # Errors
    ///
    /// Returns a store error when deletion fails.
    pub fn delete_project(&mut self, project: &ProjectId) -> Result<bool, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "DELETE FROM projects WHERE id = ?1",
            params![project.as_str()],
        )?;
        transaction.commit()?;
        Ok(changed != 0)
    }

    /// Atomically advances and returns a project's indexing generation.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown project or `u64`/`SQLite` integer overflow.
    pub fn begin_generation(&mut self, project: &ProjectId) -> Result<Generation, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = project_generation(&transaction, project)?;
        let next = current
            .value()
            .checked_add(1)
            .ok_or_else(|| StoreError::GenerationOverflow(project.clone()))?;
        let next_sql = sqlite_integer("project generation", next)?;
        transaction.execute(
            "UPDATE projects SET current_generation = ?2 WHERE id = ?1",
            params![project.as_str(), next_sql],
        )?;
        transaction.commit()?;
        Ok(Generation::new(next))
    }

    /// Inserts or refreshes a normalized file record in the current generation.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown projects, stale generations, overflow, or SQL failure.
    pub fn upsert_file(&mut self, file: &FileRecord) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_generation(&transaction, &file.id.project, file.generation)?;
        upsert_file_in(&transaction, file)?;
        transaction.commit()?;
        Ok(())
    }

    /// Transactionally replaces one file's graph using deterministic insertion order.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or persistence error. Any partial change rolls back.
    pub fn replace_file_graph(
        &mut self,
        file: &FileRecord,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> Result<ReplacementOutcome, StoreError> {
        validate_replacement(file, nodes, edges)?;
        let mut ordered_nodes = nodes.to_vec();
        ordered_nodes.sort_by(|left, right| left.id.cmp(&right.id));
        let mut ordered_edges = edges.to_vec();
        ordered_edges.sort_by(|left, right| {
            (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
                &right.source,
                &right.target,
                &right.kind,
                &right.discriminator,
            ))
        });

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_generation(&transaction, &file.id.project, file.generation)?;
        transaction.execute(
            "DELETE FROM nodes WHERE project_id = ?1 AND file_path = ?2",
            params![file.id.project.as_str(), file.id.path.as_str()],
        )?;
        upsert_file_in(&transaction, file)?;
        for node in &ordered_nodes {
            insert_node(&transaction, node)?;
        }
        for edge in &ordered_edges {
            ensure_node_exists(&transaction, &edge.project, &edge.source)?;
            ensure_node_exists(&transaction, &edge.project, &edge.target)?;
            insert_edge(&transaction, edge)?;
        }
        transaction.commit()?;
        Ok(ReplacementOutcome {
            nodes: ordered_nodes.len(),
            edges: ordered_edges.len(),
        })
    }

    /// Atomically registers and replaces one project's complete graph.
    ///
    /// Input generations are placeholders. The committed files, nodes, and edges all receive
    /// exactly one newly allocated project generation.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or persistence error. Registration, generation advancement,
    /// stale graph deletion, FTS maintenance, and insertion roll back together on failure.
    pub fn replace_project_graph(
        &mut self,
        project: &ProjectRecord,
        mut files: Vec<FileRecord>,
        mut nodes: Vec<GraphNode>,
        mut edges: Vec<GraphEdge>,
    ) -> Result<ProjectReplacementOutcome, StoreError> {
        validate_project_replacement(&project.id, &files, &nodes, &edges)?;
        files.sort_by(|left, right| left.id.path.cmp(&right.id.path));
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        edges.sort_by(|left, right| {
            (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
                &right.source,
                &right.target,
                &right.kind,
                &right.discriminator,
            ))
        });

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let initial_generation = sqlite_integer("project generation", project.generation.value())?;
        transaction.execute(
            "INSERT INTO projects(id, root_path, current_generation) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![project.id.as_str(), project.root_path, initial_generation],
        )?;
        let current = project_generation(&transaction, &project.id)?;
        let next_value = current
            .value()
            .checked_add(1)
            .ok_or_else(|| StoreError::GenerationOverflow(project.id.clone()))?;
        let generation = Generation::new(next_value);
        let generation_sql = sqlite_integer("project generation", next_value)?;
        transaction.execute(
            "UPDATE projects SET current_generation = ?2 WHERE id = ?1",
            params![project.id.as_str(), generation_sql],
        )?;

        for file in &mut files {
            file.generation = generation;
        }
        for node in &mut nodes {
            node.generation = generation;
        }
        for edge in &mut edges {
            edge.generation = generation;
        }

        transaction.execute(
            "DELETE FROM nodes WHERE project_id = ?1",
            params![project.id.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM files WHERE project_id = ?1",
            params![project.id.as_str()],
        )?;
        for file in &files {
            upsert_file_in(&transaction, file)?;
        }
        for node in &nodes {
            insert_node(&transaction, node)?;
        }
        for edge in &edges {
            ensure_node_exists(&transaction, &edge.project, &edge.source)?;
            ensure_node_exists(&transaction, &edge.project, &edge.target)?;
            insert_edge(&transaction, edge)?;
        }

        let outcome = ProjectReplacementOutcome {
            generation,
            files: files.len(),
            nodes: nodes.len(),
            edges: edges.len(),
        };
        transaction.commit()?;
        Ok(outcome)
    }

    /// Reconciles the current project generation against its complete seen-path set.
    ///
    /// Retained files are touched to `generation`; unseen files cascade-delete their graph.
    ///
    /// # Errors
    ///
    /// Returns an error for stale generations, unknown retained files, or SQL failure.
    pub fn reconcile_project(
        &mut self,
        project: &ProjectId,
        generation: Generation,
        retained: &BTreeSet<ProjectRelativePath>,
    ) -> Result<ReconcileOutcome, StoreError> {
        let generation_sql = sqlite_integer("file generation", generation.value())?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_generation(&transaction, project, generation)?;
        let existing = project_file_paths(&transaction, project)?;
        for path in retained {
            if !existing.contains(path) {
                return Err(StoreError::FileNotFound(FileId::new(
                    project.clone(),
                    path.clone(),
                )));
            }
        }

        let mut removed_files = 0;
        for path in existing.difference(retained) {
            removed_files += transaction.execute(
                "DELETE FROM files WHERE project_id = ?1 AND path = ?2",
                params![project.as_str(), path.as_str()],
            )?;
        }
        for path in retained {
            transaction.execute(
                "UPDATE files SET generation = ?3 WHERE project_id = ?1 AND path = ?2",
                params![project.as_str(), path.as_str(), generation_sql],
            )?;
        }
        transaction.commit()?;
        Ok(ReconcileOutcome { removed_files })
    }

    /// Creates an immutable recovery journal record in the prepared phase.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, project lookup, or persistence error.
    pub fn create_edit_operation(
        &mut self,
        record: &NewEditJournalRecord,
    ) -> Result<EditJournalRecord, StoreError> {
        validate_new_edit_record(record)?;
        let created_parent_paths = serde_json::to_string(&record.created_parent_paths)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        project_generation(&transaction, &record.project_id)?;
        let target_busy = transaction.query_row(
            "SELECT EXISTS(\
                 SELECT 1 FROM edit_journal \
                 WHERE project_id = ?1 AND path = ?2 \
                   AND phase NOT IN ('committed', 'rolled_back')\
             )",
            params![record.project_id.as_str(), record.path.as_str()],
            |row| row.get::<_, bool>(0),
        )?;
        if target_busy {
            return Err(StoreError::EditTargetBusy {
                project_id: record.project_id.clone(),
                path: record.path.clone(),
            });
        }
        transaction.execute(
            "INSERT INTO edit_journal(\
                 operation_id, record_version, operation_kind, project_id, path, original_hash, \
                 new_hash, temp_path, backup_path, created_parent_paths_json, phase\
             ) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'prepared')",
            params![
                record.operation_id.as_str(),
                record.operation_kind.as_str(),
                record.project_id.as_str(),
                record.path.as_str(),
                record.original_hash.map(|hash| hash.to_string()),
                record.new_hash.map(|hash| hash.to_string()),
                record.temp_path.as_ref().map(ProjectRelativePath::as_str),
                record.backup_path.as_ref().map(ProjectRelativePath::as_str),
                created_parent_paths,
            ],
        )?;
        let stored = get_edit_operation(&transaction, &record.operation_id)?
            .ok_or_else(|| StoreError::EditOperationNotFound(record.operation_id.clone()))?;
        transaction.commit()?;
        Ok(stored)
    }

    /// Advances an operation with compare-and-set semantics.
    ///
    /// Repeating a successfully persisted target phase is idempotent. Otherwise the stored phase
    /// must match `expected` and the transition must be the next forward phase or a rollback.
    ///
    /// # Errors
    ///
    /// Returns a not-found, stale-phase, invalid-transition, or persistence error.
    pub fn transition_edit_operation(
        &mut self,
        operation_id: &EditOperationId,
        expected: EditPhase,
        next: EditPhase,
    ) -> Result<EditJournalRecord, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = get_edit_operation(&transaction, operation_id)?
            .ok_or_else(|| StoreError::EditOperationNotFound(operation_id.clone()))?;
        if current.phase == next {
            transaction.commit()?;
            return Ok(current);
        }
        if current.phase != expected {
            return Err(StoreError::StaleEditPhase {
                expected,
                actual: current.phase,
            });
        }
        if !expected.can_transition_to(next) {
            return Err(StoreError::InvalidEditPhaseTransition {
                from: expected,
                to: next,
            });
        }
        let changed = transaction.execute(
            "UPDATE edit_journal \
             SET phase = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE operation_id = ?1 AND phase = ?3",
            params![operation_id.as_str(), next.as_str(), expected.as_str()],
        )?;
        if changed != 1 {
            return Err(StoreError::StaleEditPhase {
                expected,
                actual: current.phase,
            });
        }
        let updated = get_edit_operation(&transaction, operation_id)?
            .ok_or_else(|| StoreError::EditOperationNotFound(operation_id.clone()))?;
        transaction.commit()?;
        Ok(updated)
    }

    /// Replaces or clears the last recovery error for an operation.
    ///
    /// # Errors
    ///
    /// Returns a not-found or persistence error. The update is atomic.
    pub fn set_edit_operation_error(
        &mut self,
        operation_id: &EditOperationId,
        error: Option<&str>,
    ) -> Result<EditJournalRecord, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE edit_journal \
             SET last_error = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE operation_id = ?1",
            params![operation_id.as_str(), error],
        )?;
        if changed != 1 {
            return Err(StoreError::EditOperationNotFound(operation_id.clone()));
        }
        let updated = get_edit_operation(&transaction, operation_id)?
            .ok_or_else(|| StoreError::EditOperationNotFound(operation_id.clone()))?;
        transaction.commit()?;
        Ok(updated)
    }

    /// Deletes a journal record after its recovery material has been cleaned up.
    ///
    /// # Errors
    ///
    /// Returns a persistence error when deletion fails.
    pub fn delete_edit_operation(
        &mut self,
        operation_id: &EditOperationId,
    ) -> Result<bool, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "DELETE FROM edit_journal WHERE operation_id = ?1",
            params![operation_id.as_str()],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Stores an ADR for an indexed project, preserving its creation timestamp on update.
    ///
    /// # Errors
    ///
    /// Returns a typed not-found or storage error.
    pub fn store_adr(&mut self, project: &ProjectId, content: &str) -> Result<(), StoreError> {
        ensure_project_exists(&self.connection, project)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO project_summaries(project_id, content) VALUES (?1, ?2) \
             ON CONFLICT(project_id) DO UPDATE SET content = excluded.content, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![project.as_str(), content],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Deletes an ADR when one exists.
    ///
    /// # Errors
    ///
    /// Returns a storage error when deletion fails.
    pub fn delete_adr(&mut self, project: &ProjectId) -> Result<bool, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "DELETE FROM project_summaries WHERE project_id = ?1",
            params![project.as_str()],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Merges ADR sections using upstream canonical ordering and the 8,000-byte limit.
    ///
    /// # Errors
    ///
    /// Returns a typed not-found, size, or storage error.
    pub fn update_adr_sections(
        &mut self,
        project: &ProjectId,
        updates: &[AdrSection],
    ) -> Result<AdrRecord, StoreError> {
        let existing = self
            .get_adr(project)?
            .ok_or_else(|| StoreError::AdrNotFound(project.clone()))?;
        let mut sections = parse_adr_sections(&existing.content);
        for update in updates {
            if let Some(section) = sections
                .iter_mut()
                .find(|section| section.name == update.name)
            {
                section.content.clone_from(&update.content);
            } else if sections.len() < ADR_MAX_SECTIONS {
                sections.push(update.clone());
            }
        }
        let merged = render_adr_sections(&sections);
        if merged.len() > ADR_MAX_LENGTH {
            return Err(StoreError::AdrTooLarge {
                limit: ADR_MAX_LENGTH,
                actual: merged.len(),
            });
        }
        self.store_adr(project, &merged)?;
        self.get_adr(project)?
            .ok_or_else(|| StoreError::AdrNotFound(project.clone()))
    }

    /// Atomically aggregates runtime edge observations for an indexed project.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, overflow, or storage error.
    pub fn ingest_runtime_traces(
        &mut self,
        project: &ProjectId,
        traces: &[RuntimeTrace],
    ) -> Result<usize, StoreError> {
        ensure_project_exists(&self.connection, project)?;
        let mut aggregated = BTreeMap::<(String, String), u64>::new();
        for trace in traces {
            validate_runtime_trace(trace)?;
            let count = aggregated
                .entry((trace.caller.clone(), trace.callee.clone()))
                .or_default();
            *count = count
                .checked_add(trace.count)
                .ok_or(StoreError::InvalidRuntimeTrace {
                    reason: "count overflow",
                })?;
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for ((caller, callee), count) in aggregated {
            let existing = transaction
                .query_row(
                    "SELECT count FROM runtime_traces \
                     WHERE project_id = ?1 AND caller = ?2 AND callee = ?3",
                    params![project.as_str(), caller, callee],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let total = existing.map_or(Ok(count), |value| {
                sqlite_u64("runtime trace count", value)?
                    .checked_add(count)
                    .ok_or(StoreError::InvalidRuntimeTrace {
                        reason: "count overflow",
                    })
            })?;
            let total = sqlite_integer("runtime trace count", total)?;
            transaction.execute(
                "INSERT INTO runtime_traces(project_id, caller, callee, count) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(project_id, caller, callee) DO UPDATE SET \
                 count = excluded.count, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![project.as_str(), caller, callee, total],
            )?;
        }
        transaction.commit()?;
        Ok(traces.len())
    }

    /// Atomically replaces Git temporal/co-change data and enriches existing File nodes.
    ///
    /// Existing history-derived edges and temporal properties are removed before the new
    /// bounded snapshot is installed. Missing File nodes do not discard the durable history.
    ///
    /// # Errors
    ///
    /// Returns a validation, project-not-found, overflow, or storage error.
    pub fn replace_git_history(
        &mut self,
        project: &ProjectId,
        files: &[GitFileHistoryRecord],
        couplings: &[GitCoChangeRecord],
    ) -> Result<GitHistoryOutcome, StoreError> {
        ensure_project_exists(&self.connection, project)?;
        validate_git_history(files, couplings)?;
        let generation = self
            .get_project(project)?
            .ok_or_else(|| StoreError::ProjectNotFound(project.clone()))?
            .generation;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM edges WHERE project_id = ?1 AND kind = 'FILE_CHANGES_WITH'",
            params![project.as_str()],
        )?;
        transaction.execute(
            "UPDATE nodes SET properties_json = json_remove(properties_json, \
             '$.last_modified', '$.change_count') \
             WHERE project_id = ?1 AND label = 'File'",
            params![project.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM git_cochanges WHERE project_id = ?1",
            params![project.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM git_file_history WHERE project_id = ?1",
            params![project.as_str()],
        )?;

        let mut enriched_files = 0;
        for file in files {
            transaction.execute(
                "INSERT INTO git_file_history(\
                   project_id, path, change_count, last_modified\
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    project.as_str(),
                    file.path.as_str(),
                    sqlite_integer("Git change count", file.change_count)?,
                    file.last_modified,
                ],
            )?;
            enriched_files += transaction.execute(
                "UPDATE nodes SET properties_json = json_set(properties_json, \
                 '$.last_modified', ?3, '$.change_count', ?4) \
                 WHERE project_id = ?1 AND label = 'File' AND file_path = ?2",
                params![
                    project.as_str(),
                    file.path.as_str(),
                    file.last_modified,
                    sqlite_integer("Git change count", file.change_count)?,
                ],
            )?;
        }

        let mut enriched_edges = 0;
        for coupling in couplings {
            transaction.execute(
                "INSERT INTO git_cochanges(\
                   project_id, file_a, file_b, co_changes, coupling_score, last_co_change\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project.as_str(),
                    coupling.file_a.as_str(),
                    coupling.file_b.as_str(),
                    sqlite_integer("Git co-change count", coupling.co_changes)?,
                    coupling.coupling_score,
                    coupling.last_co_change,
                ],
            )?;
            let source = file_node_id(&transaction, project, &coupling.file_a)?;
            let target = file_node_id(&transaction, project, &coupling.file_b)?;
            if let (Some(source), Some(target)) = (source, target)
                && source != target
            {
                let graph_score = (coupling.coupling_score * 100.0).round() / 100.0;
                let properties = serde_json::json!({
                    "co_changes": coupling.co_changes,
                    "coupling_score": graph_score,
                    "last_co_change": coupling.last_co_change
                });
                transaction.execute(
                    "INSERT INTO edges(\
                       project_id, source_id, target_id, kind, discriminator, generation, properties_json\
                     ) VALUES (?1, ?2, ?3, 'FILE_CHANGES_WITH', '', ?4, ?5)",
                    params![
                        project.as_str(),
                        source,
                        target,
                        sqlite_integer("edge generation", generation.value())?,
                        serde_json::to_string(&properties)?,
                    ],
                )?;
                enriched_edges += 1;
            }
        }
        transaction.commit()?;
        Ok(GitHistoryOutcome {
            files: files.len(),
            couplings: couplings.len(),
            enriched_files,
            enriched_edges,
        })
    }

    /// Atomically replaces all persisted semantic vectors and structural signatures.
    ///
    /// # Errors
    ///
    /// Returns a validation, project-not-found, foreign-key, overflow, or storage error.
    pub fn replace_semantic_index(
        &mut self,
        project: &ProjectId,
        node_vectors: &[NodeVectorRecord],
        token_vectors: &[TokenVectorRecord],
        node_signatures: &[NodeSignatureRecord],
    ) -> Result<SemanticIndexOutcome, StoreError> {
        validate_semantic_index(node_vectors, token_vectors, node_signatures)?;
        ensure_project_exists(&self.connection, project)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM node_vectors WHERE project_id = ?1",
            params![project.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM token_vectors WHERE project_id = ?1",
            params![project.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM node_signatures WHERE project_id = ?1",
            params![project.as_str()],
        )?;

        for record in node_vectors {
            transaction.execute(
                "INSERT INTO node_vectors(project_id, node_id, vector) VALUES (?1, ?2, ?3)",
                params![
                    project.as_str(),
                    record.node_id.as_str(),
                    record.vector.to_blob(),
                ],
            )?;
        }
        for record in token_vectors {
            transaction.execute(
                "INSERT INTO token_vectors(project_id, token, vector, idf_milli) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    project.as_str(),
                    record.token,
                    record.vector.to_blob(),
                    i64::from(record.idf_milli),
                ],
            )?;
        }
        for record in node_signatures {
            transaction.execute(
                "INSERT INTO node_signatures(\
                   project_id, node_id, minhash_hex, ast_profile\
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    project.as_str(),
                    record.node_id.as_str(),
                    record.minhash_hex,
                    record.ast_profile,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(SemanticIndexOutcome {
            node_vectors: node_vectors.len(),
            token_vectors: token_vectors.len(),
            node_signatures: node_signatures.len(),
        })
    }
}

macro_rules! impl_read_api {
    ($type:ty) => {
        impl $type {
            /// Returns versioned schema metadata.
            ///
            /// # Errors
            ///
            /// Returns a store error when schema introspection fails.
            pub fn schema_info(&self) -> Result<SchemaInfo, StoreError> {
                schema::inspect(&self.connection)
            }

            /// Returns effective connection pragmas.
            ///
            /// # Errors
            ///
            /// Returns a store error when a pragma cannot be read.
            pub fn connection_settings(&self) -> Result<ConnectionSettings, StoreError> {
                connection_settings(&self.connection)
            }

            /// Finds a project registry record by exact case-sensitive ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_project(
                &self,
                project: &ProjectId,
            ) -> Result<Option<ProjectRecord>, StoreError> {
                get_project(&self.connection, project)
            }

            /// Lists projects in deterministic ID order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, StoreError> {
                list_projects(&self.connection)
            }

            /// Finds the project ADR when one exists.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_adr(&self, project: &ProjectId) -> Result<Option<AdrRecord>, StoreError> {
                get_adr(&self.connection, project)
            }

            /// Lists aggregated runtime traces in stable caller/callee order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_runtime_traces(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<RuntimeTraceRecord>, StoreError> {
                list_runtime_traces(&self.connection, project)
            }

            /// Lists temporal Git metadata in deterministic path order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_git_file_history(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<GitFileHistoryRecord>, StoreError> {
                list_git_file_history(&self.connection, project)
            }

            /// Lists co-change relationships in deterministic pair order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_git_cochanges(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<GitCoChangeRecord>, StoreError> {
                list_git_cochanges(&self.connection, project)
            }

            /// Returns files historically coupled to one path, strongest first.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn coupled_files(
                &self,
                project: &ProjectId,
                path: &ProjectRelativePath,
            ) -> Result<Vec<GitCoChangeRecord>, StoreError> {
                coupled_files(&self.connection, project, path)
            }

            /// Finds a normalized file record by compound identity.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, StoreError> {
                get_file(&self.connection, file)
            }

            /// Lists a project's files in deterministic path order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, StoreError> {
                list_files(&self.connection, project)
            }

            /// Lists all project nodes by qualified name and stable ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, StoreError> {
                list_nodes(&self.connection, project)
            }

            /// Lists all project edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, StoreError> {
                list_edges(&self.connection, project)
            }

            /// Lists a file's nodes in deterministic node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, StoreError> {
                nodes_for_file(&self.connection, file)
            }

            /// Finds a graph node by stable ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_node(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<GraphNode>, StoreError> {
                get_node(&self.connection, project, node)
            }

            /// Finds a graph node by exact qualified name.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn node_by_qualified_name(
                &self,
                project: &ProjectId,
                qualified_name: &QualifiedName,
            ) -> Result<Option<GraphNode>, StoreError> {
                node_by_qualified_name(&self.connection, project, qualified_name)
            }

            /// Lists outbound edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn edges_from(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Vec<GraphEdge>, StoreError> {
                edges_from(&self.connection, project, node)
            }

            /// Lists inbound edges in deterministic identity order.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn edges_to(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Vec<GraphEdge>, StoreError> {
                edges_to(&self.connection, project, node)
            }

            /// Runs a project-scoped `FTS5` query ordered by rank and node ID.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax or read/decode failure.
            pub fn search_nodes(
                &self,
                project: &ProjectId,
                query: &str,
                limit: usize,
            ) -> Result<Vec<SearchHit>, StoreError> {
                search_nodes_page(&self.connection, project, query, limit, 0)
            }

            /// Runs a deterministic project-scoped `FTS5` page.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax, numeric overflow, or decode failure.
            pub fn search_nodes_page(
                &self,
                project: &ProjectId,
                query: &str,
                limit: usize,
                offset: usize,
            ) -> Result<Vec<SearchHit>, StoreError> {
                search_nodes_page(&self.connection, project, query, limit, offset)
            }

            /// Counts project-scoped `FTS5` matches without materializing nodes.
            ///
            /// # Errors
            ///
            /// Returns a store error for invalid `FTS5` syntax or read failure.
            pub fn count_search_nodes(
                &self,
                project: &ProjectId,
                query: &str,
            ) -> Result<u64, StoreError> {
                count_search_nodes(&self.connection, project, query)
            }

            /// Lists persisted node vectors in stable node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_node_vectors(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<NodeVectorRecord>, StoreError> {
                list_node_vectors(&self.connection, project)
            }

            /// Finds one persisted node vector by stable node ID.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_node_vector(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<NodeVectorRecord>, StoreError> {
                get_node_vector(&self.connection, project, node)
            }

            /// Finds one enriched token vector by exact case-sensitive token.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_token_vector(
                &self,
                project: &ProjectId,
                token: &str,
            ) -> Result<Option<TokenVectorRecord>, StoreError> {
                get_token_vector(&self.connection, project, token)
            }

            /// Lists structural signatures in stable node-ID order.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn list_node_signatures(
                &self,
                project: &ProjectId,
            ) -> Result<Vec<NodeSignatureRecord>, StoreError> {
                list_node_signatures(&self.connection, project)
            }

            /// Finds one structural signature by stable node ID.
            ///
            /// # Errors
            ///
            /// Returns a storage or decode error.
            pub fn get_node_signature(
                &self,
                project: &ProjectId,
                node: &NodeId,
            ) -> Result<Option<NodeSignatureRecord>, StoreError> {
                get_node_signature(&self.connection, project, node)
            }

            /// Counts normalized graph records for a project.
            ///
            /// # Errors
            ///
            /// Returns a store error when any count query fails.
            pub fn counts(&self, project: &ProjectId) -> Result<GraphCounts, StoreError> {
                counts(&self.connection, project)
            }

            /// Finds one durable edit journal record by operation ID.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn get_edit_operation(
                &self,
                operation_id: &EditOperationId,
            ) -> Result<Option<EditJournalRecord>, StoreError> {
                get_edit_operation(&self.connection, operation_id)
            }

            /// Lists recoverable edit operations in deterministic creation order.
            ///
            /// Committed and rolled-back records are terminal and therefore excluded.
            ///
            /// # Errors
            ///
            /// Returns a store error when reading or decoding fails.
            pub fn list_incomplete_edit_operations(
                &self,
            ) -> Result<Vec<EditJournalRecord>, StoreError> {
                list_incomplete_edit_operations(&self.connection)
            }
        }
    };
}

impl_read_api!(Store);
impl_read_api!(QueryStore);

fn validate_semantic_index(
    node_vectors: &[NodeVectorRecord],
    token_vectors: &[TokenVectorRecord],
    node_signatures: &[NodeSignatureRecord],
) -> Result<(), StoreError> {
    let mut vector_nodes = BTreeSet::new();
    for record in node_vectors {
        if !vector_nodes.insert(&record.node_id) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate node vector for {:?}", record.node_id),
            });
        }
    }

    let mut tokens = BTreeSet::new();
    for record in token_vectors {
        if record.token.is_empty() || record.token.contains('\0') {
            return Err(StoreError::InvalidSemanticRecord {
                reason: "token must be non-empty and contain no NUL bytes".to_owned(),
            });
        }
        if !tokens.insert(&record.token) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate token vector for {:?}", record.token),
            });
        }
    }

    let mut signature_nodes = BTreeSet::new();
    for record in node_signatures {
        if !signature_nodes.insert(&record.node_id) {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("duplicate node signature for {:?}", record.node_id),
            });
        }
        if record.minhash_hex.len() != MINHASH_SIGNATURE_HEX_LEN
            || !record
                .minhash_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!(
                    "MinHash signature for {:?} must contain {MINHASH_SIGNATURE_HEX_LEN} hex digits",
                    record.node_id
                ),
            });
        }
        if record
            .ast_profile
            .as_ref()
            .is_some_and(|profile| profile.contains('\0'))
        {
            return Err(StoreError::InvalidSemanticRecord {
                reason: format!("AST profile for {:?} contains a NUL byte", record.node_id),
            });
        }
    }
    Ok(())
}

fn list_node_vectors(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<NodeVectorRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT node_id, vector FROM node_vectors \
         WHERE project_id = ?1 ORDER BY node_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    rows.map(|row| {
        let (node_id, vector) = row?;
        Ok(NodeVectorRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node vector ID"))?,
            vector: StoredVector::from_blob(vector, "node vector")?,
        })
    })
    .collect()
}

fn get_node_vector(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Option<NodeVectorRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, vector FROM node_vectors \
             WHERE project_id = ?1 AND node_id = ?2",
            params![project.as_str(), node.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    raw.map(|(node_id, vector)| {
        Ok(NodeVectorRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node vector ID"))?,
            vector: StoredVector::from_blob(vector, "node vector")?,
        })
    })
    .transpose()
}

fn get_token_vector(
    connection: &Connection,
    project: &ProjectId,
    token: &str,
) -> Result<Option<TokenVectorRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT token, vector, idf_milli FROM token_vectors \
             WHERE project_id = ?1 AND token = ?2",
            params![project.as_str(), token],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(token, vector, idf_milli)| {
        Ok(TokenVectorRecord {
            token,
            vector: StoredVector::from_blob(vector, "token vector")?,
            idf_milli: u32::try_from(sqlite_u64("token vector IDF", idf_milli)?).map_err(|_| {
                StoreError::CorruptData {
                    field: "token vector IDF",
                    reason: format!("value {idf_milli} does not fit u32"),
                }
            })?,
        })
    })
    .transpose()
}

fn list_node_signatures(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<NodeSignatureRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT node_id, minhash_hex, ast_profile FROM node_signatures \
         WHERE project_id = ?1 ORDER BY node_id COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (node_id, minhash_hex, ast_profile) = row?;
        Ok(NodeSignatureRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node signature ID"))?,
            minhash_hex,
            ast_profile,
        })
    })
    .collect()
}

fn get_node_signature(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Option<NodeSignatureRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, minhash_hex, ast_profile FROM node_signatures \
             WHERE project_id = ?1 AND node_id = ?2",
            params![project.as_str(), node.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(node_id, minhash_hex, ast_profile)| {
        Ok(NodeSignatureRecord {
            node_id: NodeId::new(node_id).map_err(corrupt_graph("node signature ID"))?,
            minhash_hex,
            ast_profile,
        })
    })
    .transpose()
}

fn configure_writable(connection: &Connection, in_memory: bool) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    if in_memory {
        connection.pragma_update(None, "synchronous", "OFF")?;
    } else {
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
    }
    Ok(())
}

fn configure_read_only(connection: &Connection) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "query_only", true)?;
    connection.query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))?;
    Ok(())
}

fn connection_settings(connection: &Connection) -> Result<ConnectionSettings, StoreError> {
    let foreign_keys = connection.pragma_query_value(None, "foreign_keys", |row| {
        row.get::<_, i64>(0).map(|value| value != 0)
    })?;
    let journal_mode = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let synchronous = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    let busy_timeout_ms =
        connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))?;
    let query_only = connection.pragma_query_value(None, "query_only", |row| {
        row.get::<_, i64>(0).map(|value| value != 0)
    })?;
    Ok(ConnectionSettings {
        foreign_keys,
        journal_mode,
        synchronous,
        busy_timeout_ms: sqlite_u64("busy timeout", busy_timeout_ms)?,
        query_only,
    })
}

fn validate_new_edit_record(record: &NewEditJournalRecord) -> Result<(), StoreError> {
    let hashes_match_kind = match record.operation_kind {
        EditOperationKind::Create => record.original_hash.is_none() && record.new_hash.is_some(),
        EditOperationKind::Update => record.original_hash.is_some() && record.new_hash.is_some(),
        EditOperationKind::Delete => record.original_hash.is_some() && record.new_hash.is_none(),
    };
    if !hashes_match_kind {
        return Err(StoreError::InvalidEditJournalRecord {
            reason: "hash presence does not match operation kind",
        });
    }
    let unique_parents: BTreeSet<_> = record.created_parent_paths.iter().collect();
    if unique_parents.len() != record.created_parent_paths.len() {
        return Err(StoreError::InvalidEditJournalRecord {
            reason: "created parent paths must be unique",
        });
    }
    Ok(())
}

#[derive(Debug)]
struct RawEditJournalRecord {
    operation_id: String,
    record_version: i64,
    operation_kind: String,
    project_id: String,
    path: String,
    original_hash: Option<String>,
    new_hash: Option<String>,
    temp_path: Option<String>,
    backup_path: Option<String>,
    created_parent_paths: String,
    phase: String,
    created_at: String,
    updated_at: String,
    last_error: Option<String>,
}

fn raw_edit_operation(row: &Row<'_>) -> rusqlite::Result<RawEditJournalRecord> {
    Ok(RawEditJournalRecord {
        operation_id: row.get(0)?,
        record_version: row.get(1)?,
        operation_kind: row.get(2)?,
        project_id: row.get(3)?,
        path: row.get(4)?,
        original_hash: row.get(5)?,
        new_hash: row.get(6)?,
        temp_path: row.get(7)?,
        backup_path: row.get(8)?,
        created_parent_paths: row.get(9)?,
        phase: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        last_error: row.get(13)?,
    })
}

fn get_edit_operation(
    connection: &Connection,
    operation_id: &EditOperationId,
) -> Result<Option<EditJournalRecord>, StoreError> {
    let sql = format!("SELECT {EDIT_JOURNAL_COLUMNS} FROM edit_journal WHERE operation_id = ?1");
    let raw = connection
        .query_row(&sql, params![operation_id.as_str()], raw_edit_operation)
        .optional()?;
    raw.map(edit_operation_from_raw).transpose()
}

fn list_incomplete_edit_operations(
    connection: &Connection,
) -> Result<Vec<EditJournalRecord>, StoreError> {
    let sql = format!(
        "SELECT {EDIT_JOURNAL_COLUMNS} FROM edit_journal \
         WHERE phase NOT IN ('committed', 'rolled_back') \
         ORDER BY created_at COLLATE BINARY, operation_id COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], raw_edit_operation)?;
    rows.map(|row| edit_operation_from_raw(row?)).collect()
}

fn edit_operation_from_raw(raw: RawEditJournalRecord) -> Result<EditJournalRecord, StoreError> {
    let operation_id =
        EditOperationId::new(raw.operation_id).map_err(|_| StoreError::CorruptData {
            field: "edit operation ID",
            reason: "empty or NUL-containing value".to_owned(),
        })?;
    let record_version_u64 = sqlite_u64("edit journal record version", raw.record_version)?;
    let record_version =
        u32::try_from(record_version_u64).map_err(|_| StoreError::CorruptData {
            field: "edit journal record version",
            reason: format!("value {record_version_u64} does not fit u32"),
        })?;
    if record_version != 1 {
        return Err(StoreError::CorruptData {
            field: "edit journal record version",
            reason: format!("unsupported version {record_version}"),
        });
    }
    let project_id =
        ProjectId::new(raw.project_id).map_err(corrupt_domain("edit journal project ID"))?;
    let path = ProjectRelativePath::new(raw.path).map_err(corrupt_syntax("edit journal path"))?;
    let original_hash = stored_optional_hash(raw.original_hash, "edit journal original hash")?;
    let new_hash = stored_optional_hash(raw.new_hash, "edit journal new hash")?;
    let temp_path = stored_optional_path(raw.temp_path, "edit journal temp path")?;
    let backup_path = stored_optional_path(raw.backup_path, "edit journal backup path")?;
    let created_parent_paths: Vec<ProjectRelativePath> =
        serde_json::from_str(&raw.created_parent_paths)?;
    Ok(EditJournalRecord {
        operation_id,
        record_version,
        operation_kind: EditOperationKind::from_stored(&raw.operation_kind)?,
        project_id,
        path,
        original_hash,
        new_hash,
        temp_path,
        backup_path,
        created_parent_paths,
        phase: EditPhase::from_stored(&raw.phase)?,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        last_error: raw.last_error,
    })
}

fn stored_optional_hash(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<ContentHash>, StoreError> {
    value
        .map(|hash| ContentHash::from_str(&hash).map_err(corrupt_syntax(field)))
        .transpose()
}

fn stored_optional_path(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<ProjectRelativePath>, StoreError> {
    value
        .map(|path| ProjectRelativePath::new(path).map_err(corrupt_syntax(field)))
        .transpose()
}

fn project_generation(
    transaction: &Transaction<'_>,
    project: &ProjectId,
) -> Result<Generation, StoreError> {
    let value = transaction
        .query_row(
            "SELECT current_generation FROM projects WHERE id = ?1",
            params![project.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::ProjectNotFound(project.clone()))?;
    Ok(Generation::new(sqlite_u64("project generation", value)?))
}

fn ensure_generation(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    actual: Generation,
) -> Result<(), StoreError> {
    let expected = project_generation(transaction, project)?;
    if expected != actual {
        return Err(StoreError::GenerationMismatch { expected, actual });
    }
    Ok(())
}

fn upsert_file_in(transaction: &Transaction<'_>, file: &FileRecord) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO files(project_id, path, content_hash, generation, modified_ns, byte_len) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(project_id, path) DO UPDATE SET \
           content_hash = excluded.content_hash, generation = excluded.generation, \
           modified_ns = excluded.modified_ns, byte_len = excluded.byte_len",
        params![
            file.id.project.as_str(),
            file.id.path.as_str(),
            file.content_hash.to_string(),
            sqlite_integer("file generation", file.generation.value())?,
            sqlite_integer("file modified_ns", file.modified_ns)?,
            sqlite_integer("file byte_len", file.byte_len)?,
        ],
    )?;
    Ok(())
}

fn validate_replacement(
    file: &FileRecord,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    let mut node_ids = BTreeSet::new();
    let mut qualified_names = BTreeSet::new();
    for node in nodes {
        if node.project != file.id.project {
            return Err(StoreError::ProjectMismatch {
                expected: file.id.project.clone(),
                actual: node.project.clone(),
            });
        }
        if node.generation != file.generation {
            return Err(StoreError::GenerationMismatch {
                expected: file.generation,
                actual: node.generation,
            });
        }
        if node.file_path.as_ref() != Some(&file.id.path) {
            return Err(StoreError::FileMismatch {
                expected: file.id.path.clone(),
                actual: node.file_path.clone(),
            });
        }
        if !node_ids.insert(node.id.clone()) {
            return Err(StoreError::DuplicateNodeId(node.id.clone()));
        }
        if !qualified_names.insert(node.qualified_name.clone()) {
            return Err(StoreError::DuplicateQualifiedName(
                node.qualified_name.clone(),
            ));
        }
    }

    let mut edge_ids = BTreeSet::new();
    for edge in edges {
        if edge.project != file.id.project {
            return Err(StoreError::ProjectMismatch {
                expected: file.id.project.clone(),
                actual: edge.project.clone(),
            });
        }
        if edge.generation != file.generation {
            return Err(StoreError::GenerationMismatch {
                expected: file.generation,
                actual: edge.generation,
            });
        }
        if !edge_ids.insert((
            edge.source.clone(),
            edge.target.clone(),
            edge.kind.clone(),
            edge.discriminator.clone(),
        )) {
            return Err(StoreError::DuplicateEdge);
        }
    }
    Ok(())
}

fn validate_project_replacement(
    project: &ProjectId,
    files: &[FileRecord],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    let mut file_paths = BTreeSet::new();
    for file in files {
        if file.id.project != *project {
            return Err(StoreError::ProjectMismatch {
                expected: project.clone(),
                actual: file.id.project.clone(),
            });
        }
        if !file_paths.insert(file.id.path.clone()) {
            return Err(StoreError::DuplicateFilePath(file.id.path.clone()));
        }
    }

    let mut node_ids = BTreeSet::new();
    let mut qualified_names = BTreeSet::new();
    for node in nodes {
        if node.project != *project {
            return Err(StoreError::ProjectMismatch {
                expected: project.clone(),
                actual: node.project.clone(),
            });
        }
        if let Some(path) = &node.file_path
            && !file_paths.contains(path)
        {
            return Err(StoreError::FileNotFound(FileId::new(
                project.clone(),
                path.clone(),
            )));
        }
        if !node_ids.insert(node.id.clone()) {
            return Err(StoreError::DuplicateNodeId(node.id.clone()));
        }
        if !qualified_names.insert(node.qualified_name.clone()) {
            return Err(StoreError::DuplicateQualifiedName(
                node.qualified_name.clone(),
            ));
        }
    }

    let mut edge_ids = BTreeSet::new();
    for edge in edges {
        if edge.project != *project {
            return Err(StoreError::ProjectMismatch {
                expected: project.clone(),
                actual: edge.project.clone(),
            });
        }
        if !node_ids.contains(&edge.source) {
            return Err(StoreError::MissingNode {
                node_id: edge.source.clone(),
            });
        }
        if !node_ids.contains(&edge.target) {
            return Err(StoreError::MissingNode {
                node_id: edge.target.clone(),
            });
        }
        if !edge_ids.insert((
            edge.source.clone(),
            edge.target.clone(),
            edge.kind.clone(),
            edge.discriminator.clone(),
        )) {
            return Err(StoreError::DuplicateEdge);
        }
    }
    Ok(())
}

fn insert_node(transaction: &Transaction<'_>, node: &GraphNode) -> Result<(), StoreError> {
    let span = node.source_span.map(sql_span).transpose()?;
    let (start_byte, end_byte, start_row, start_column, end_row, end_column) =
        span.map_or((None, None, None, None, None, None), |values| {
            (
                Some(values.0),
                Some(values.1),
                Some(values.2),
                Some(values.3),
                Some(values.4),
                Some(values.5),
            )
        });
    transaction.execute(
        "INSERT INTO nodes(\
           project_id, node_id, label, name, qualified_name, file_path, \
           start_byte, end_byte, start_row, start_column, end_row, end_column, \
           generation, properties_json\
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            node.project.as_str(),
            node.id.as_str(),
            node.label.as_str(),
            node.name,
            node.qualified_name.as_str(),
            node.file_path.as_ref().map(ProjectRelativePath::as_str),
            start_byte,
            end_byte,
            start_row,
            start_column,
            end_row,
            end_column,
            sqlite_integer("node generation", node.generation.value())?,
            serde_json::to_string(&node.properties)?,
        ],
    )?;
    Ok(())
}

fn ensure_node_exists(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    node: &NodeId,
) -> Result<(), StoreError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE project_id = ?1 AND node_id = ?2)",
        params![project.as_str(), node.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(StoreError::MissingNode {
            node_id: node.clone(),
        });
    }
    Ok(())
}

fn insert_edge(transaction: &Transaction<'_>, edge: &GraphEdge) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO edges(\
           project_id, source_id, target_id, kind, discriminator, generation, properties_json\
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            edge.project.as_str(),
            edge.source.as_str(),
            edge.target.as_str(),
            edge.kind.as_str(),
            edge.discriminator.as_str(),
            sqlite_integer("edge generation", edge.generation.value())?,
            serde_json::to_string(&edge.properties)?,
        ],
    )?;
    Ok(())
}

fn project_file_paths(
    transaction: &Transaction<'_>,
    project: &ProjectId,
) -> Result<BTreeSet<ProjectRelativePath>, StoreError> {
    let mut statement =
        transaction.prepare("SELECT path FROM files WHERE project_id = ?1 ORDER BY path")?;
    let rows = statement.query_map(params![project.as_str()], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let value = row?;
        ProjectRelativePath::new(value).map_err(corrupt_syntax("file path"))
    })
    .collect()
}

fn ensure_project_exists(connection: &Connection, project: &ProjectId) -> Result<(), StoreError> {
    if get_project(connection, project)?.is_none() {
        return Err(StoreError::ProjectNotFound(project.clone()));
    }
    Ok(())
}

fn get_adr(connection: &Connection, project: &ProjectId) -> Result<Option<AdrRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT project_id, content, created_at, updated_at \
             FROM project_summaries WHERE project_id = ?1",
            params![project.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(project, content, created_at, updated_at)| {
        Ok(AdrRecord {
            project: ProjectId::new(project).map_err(corrupt_domain("ADR project ID"))?,
            content,
            created_at,
            updated_at,
        })
    })
    .transpose()
}

fn list_runtime_traces(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<RuntimeTraceRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT project_id, caller, callee, count, created_at, updated_at \
         FROM runtime_traces WHERE project_id = ?1 ORDER BY caller, callee",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    rows.map(|row| {
        let (project, source, target, count, created_at, updated_at) = row?;
        Ok(RuntimeTraceRecord {
            project: ProjectId::new(project).map_err(corrupt_domain("runtime trace project ID"))?,
            caller: source,
            callee: target,
            count: sqlite_u64("runtime trace count", count)?,
            created_at,
            updated_at,
        })
    })
    .collect()
}

fn list_git_file_history(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<GitFileHistoryRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT path, change_count, last_modified FROM git_file_history \
         WHERE project_id = ?1 ORDER BY path COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (path, change_count, last_modified) = row?;
        Ok(GitFileHistoryRecord {
            path: ProjectRelativePath::new(path).map_err(corrupt_syntax("Git history path"))?,
            change_count: sqlite_u64("Git change count", change_count)?,
            last_modified,
        })
    })
    .collect()
}

fn list_git_cochanges(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<GitCoChangeRecord>, StoreError> {
    git_cochanges_where(connection, project, None)
}

fn coupled_files(
    connection: &Connection,
    project: &ProjectId,
    path: &ProjectRelativePath,
) -> Result<Vec<GitCoChangeRecord>, StoreError> {
    git_cochanges_where(connection, project, Some(path))
}

fn git_cochanges_where(
    connection: &Connection,
    project: &ProjectId,
    path: Option<&ProjectRelativePath>,
) -> Result<Vec<GitCoChangeRecord>, StoreError> {
    let (sql, path_value) = path.map_or_else(
        || {
            (
                "SELECT file_a, file_b, co_changes, coupling_score, last_co_change \
                 FROM git_cochanges WHERE project_id = ?1 \
                 ORDER BY file_a COLLATE BINARY, file_b COLLATE BINARY",
                None,
            )
        },
        |path| {
            (
                "SELECT file_a, file_b, co_changes, coupling_score, last_co_change \
                 FROM git_cochanges WHERE project_id = ?1 AND (file_a = ?2 OR file_b = ?2) \
                 ORDER BY coupling_score DESC, co_changes DESC, file_a COLLATE BINARY, \
                          file_b COLLATE BINARY",
                Some(path.as_str()),
            )
        },
    );
    let mut statement = connection.prepare(sql)?;
    let decode = |row: &Row<'_>| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    };
    let rows = if let Some(path) = path_value {
        statement.query_map(params![project.as_str(), path], decode)?
    } else {
        statement.query_map(params![project.as_str()], decode)?
    };
    rows.map(|row| {
        let (file_a, file_b, co_changes, coupling_score, last_co_change) = row?;
        Ok(GitCoChangeRecord {
            file_a: ProjectRelativePath::new(file_a)
                .map_err(corrupt_syntax("Git co-change file_a"))?,
            file_b: ProjectRelativePath::new(file_b)
                .map_err(corrupt_syntax("Git co-change file_b"))?,
            co_changes: sqlite_u64("Git co-change count", co_changes)?,
            coupling_score,
            last_co_change,
        })
    })
    .collect()
}

fn validate_git_history(
    files: &[GitFileHistoryRecord],
    couplings: &[GitCoChangeRecord],
) -> Result<(), StoreError> {
    let mut paths = BTreeSet::new();
    for file in files {
        if file.change_count == 0 || file.last_modified < 0 {
            return Err(StoreError::InvalidGitHistory {
                reason: "file count must be positive and timestamp non-negative",
            });
        }
        if !paths.insert(file.path.clone()) {
            return Err(StoreError::InvalidGitHistory {
                reason: "duplicate file history path",
            });
        }
    }
    let mut pairs = BTreeSet::new();
    for coupling in couplings {
        if coupling.file_a >= coupling.file_b
            || coupling.co_changes == 0
            || coupling.last_co_change < 0
            || !coupling.coupling_score.is_finite()
            || !(0.0..=1.0).contains(&coupling.coupling_score)
        {
            return Err(StoreError::InvalidGitHistory {
                reason: "invalid co-change pair",
            });
        }
        if !pairs.insert((coupling.file_a.clone(), coupling.file_b.clone())) {
            return Err(StoreError::InvalidGitHistory {
                reason: "duplicate co-change pair",
            });
        }
    }
    Ok(())
}

fn file_node_id(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    path: &ProjectRelativePath,
) -> Result<Option<String>, StoreError> {
    Ok(transaction
        .query_row(
            "SELECT node_id FROM nodes WHERE project_id = ?1 AND label = 'File' \
             AND file_path = ?2 ORDER BY node_id COLLATE BINARY LIMIT 1",
            params![project.as_str(), path.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

fn validate_runtime_trace(trace: &RuntimeTrace) -> Result<(), StoreError> {
    if trace.caller.is_empty() || trace.callee.is_empty() {
        return Err(StoreError::InvalidRuntimeTrace {
            reason: "caller and callee must be non-empty",
        });
    }
    if trace.caller.contains('\0') || trace.callee.contains('\0') {
        return Err(StoreError::InvalidRuntimeTrace {
            reason: "caller and callee must not contain NUL bytes",
        });
    }
    if trace.count == 0 {
        return Err(StoreError::InvalidRuntimeTrace {
            reason: "count must be positive",
        });
    }
    Ok(())
}

fn get_project(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Option<ProjectRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT id, root_path, current_generation FROM projects WHERE id = ?1",
            params![project.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    raw.map(project_from_raw).transpose()
}

fn list_projects(connection: &Connection) -> Result<Vec<ProjectRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id, root_path, current_generation FROM projects ORDER BY id COLLATE BINARY",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.map(|row| project_from_raw(row?)).collect()
}

fn project_from_raw(raw: (String, String, i64)) -> Result<ProjectRecord, StoreError> {
    let id = ProjectId::new(raw.0).map_err(corrupt_domain("project ID"))?;
    let mut project = ProjectRecord::new(id, raw.1).map_err(corrupt_graph("project root path"))?;
    project.generation = Generation::new(sqlite_u64("project generation", raw.2)?);
    Ok(project)
}

#[derive(Debug)]
struct RawFile {
    project: String,
    path: String,
    hash: String,
    generation: i64,
    modified_ns: i64,
    byte_len: i64,
}

fn get_file(connection: &Connection, file: &FileId) -> Result<Option<FileRecord>, StoreError> {
    let raw = connection
        .query_row(
            "SELECT project_id, path, content_hash, generation, modified_ns, byte_len \
             FROM files WHERE project_id = ?1 AND path = ?2",
            params![file.project.as_str(), file.path.as_str()],
            raw_file,
        )
        .optional()?;
    raw.map(file_from_raw).transpose()
}

fn list_files(connection: &Connection, project: &ProjectId) -> Result<Vec<FileRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT project_id, path, content_hash, generation, modified_ns, byte_len \
         FROM files WHERE project_id = ?1 ORDER BY path COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], raw_file)?;
    rows.map(|row| file_from_raw(row?)).collect()
}

fn list_nodes(connection: &Connection, project: &ProjectId) -> Result<Vec<GraphNode>, StoreError> {
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes WHERE project_id = ?1 \
         ORDER BY qualified_name COLLATE BINARY, node_id COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![project.as_str()], raw_node)?;
    rows.map(|row| node_from_raw(row?)).collect()
}

fn list_edges(connection: &Connection, project: &ProjectId) -> Result<Vec<GraphEdge>, StoreError> {
    let sql = format!(
        "SELECT {EDGE_COLUMNS} FROM edges WHERE project_id = ?1 \
         ORDER BY source_id COLLATE BINARY, target_id COLLATE BINARY, \
         kind COLLATE BINARY, discriminator COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![project.as_str()], raw_edge)?;
    rows.map(|row| edge_from_raw(row?)).collect()
}

fn raw_file(row: &Row<'_>) -> rusqlite::Result<RawFile> {
    Ok(RawFile {
        project: row.get(0)?,
        path: row.get(1)?,
        hash: row.get(2)?,
        generation: row.get(3)?,
        modified_ns: row.get(4)?,
        byte_len: row.get(5)?,
    })
}

fn file_from_raw(raw: RawFile) -> Result<FileRecord, StoreError> {
    let project = ProjectId::new(raw.project).map_err(corrupt_domain("file project ID"))?;
    let path = ProjectRelativePath::new(raw.path).map_err(corrupt_syntax("file path"))?;
    let hash = ContentHash::from_str(&raw.hash).map_err(corrupt_syntax("content hash"))?;
    Ok(FileRecord::new(
        FileId::new(project, path),
        hash,
        Generation::new(sqlite_u64("file generation", raw.generation)?),
        sqlite_u64("file modified_ns", raw.modified_ns)?,
        sqlite_u64("file byte_len", raw.byte_len)?,
    ))
}

#[derive(Debug)]
struct RawNode {
    project: String,
    id: String,
    label: String,
    name: String,
    qualified_name: String,
    file_path: Option<String>,
    span: [Option<i64>; 6],
    generation: i64,
    properties: String,
}

fn raw_node(row: &Row<'_>) -> rusqlite::Result<RawNode> {
    Ok(RawNode {
        project: row.get(0)?,
        id: row.get(1)?,
        label: row.get(2)?,
        name: row.get(3)?,
        qualified_name: row.get(4)?,
        file_path: row.get(5)?,
        span: [
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
        ],
        generation: row.get(12)?,
        properties: row.get(13)?,
    })
}

fn node_from_raw(raw: RawNode) -> Result<GraphNode, StoreError> {
    let project = ProjectId::new(raw.project).map_err(corrupt_domain("node project ID"))?;
    let id = NodeId::new(raw.id).map_err(corrupt_graph("node ID"))?;
    let label = NodeLabel::new(raw.label).map_err(corrupt_graph("node label"))?;
    let qualified_name =
        QualifiedName::new(raw.qualified_name).map_err(corrupt_graph("qualified name"))?;
    let file_path = raw
        .file_path
        .map(|path| ProjectRelativePath::new(path).map_err(corrupt_syntax("node file path")))
        .transpose()?;
    let source_span = source_span_from_raw(raw.span)?;
    let properties: BTreeMap<String, Value> = serde_json::from_str(&raw.properties)?;
    GraphNode::new(
        project,
        id,
        label,
        raw.name,
        qualified_name,
        file_path,
        source_span,
        Generation::new(sqlite_u64("node generation", raw.generation)?),
    )
    .map(|node| node.with_properties(properties))
    .map_err(corrupt_graph("node"))
}

fn nodes_for_file(connection: &Connection, file: &FileId) -> Result<Vec<GraphNode>, StoreError> {
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE project_id = ?1 AND file_path = ?2 ORDER BY node_id COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![file.project.as_str(), file.path.as_str()], raw_node)?;
    rows.map(|row| node_from_raw(row?)).collect()
}

fn get_node(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Option<GraphNode>, StoreError> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE project_id = ?1 AND node_id = ?2");
    connection
        .query_row(&sql, params![project.as_str(), node.as_str()], raw_node)
        .optional()?
        .map(node_from_raw)
        .transpose()
}

fn node_by_qualified_name(
    connection: &Connection,
    project: &ProjectId,
    qualified_name: &QualifiedName,
) -> Result<Option<GraphNode>, StoreError> {
    let sql =
        format!("SELECT {NODE_COLUMNS} FROM nodes WHERE project_id = ?1 AND qualified_name = ?2");
    connection
        .query_row(
            &sql,
            params![project.as_str(), qualified_name.as_str()],
            raw_node,
        )
        .optional()?
        .map(node_from_raw)
        .transpose()
}

#[derive(Debug)]
struct RawEdge {
    project: String,
    source: String,
    target: String,
    kind: String,
    discriminator: String,
    generation: i64,
    properties: String,
}

fn raw_edge(row: &Row<'_>) -> rusqlite::Result<RawEdge> {
    Ok(RawEdge {
        project: row.get(0)?,
        source: row.get(1)?,
        target: row.get(2)?,
        kind: row.get(3)?,
        discriminator: row.get(4)?,
        generation: row.get(5)?,
        properties: row.get(6)?,
    })
}

fn edge_from_raw(raw: RawEdge) -> Result<GraphEdge, StoreError> {
    let project = ProjectId::new(raw.project).map_err(corrupt_domain("edge project ID"))?;
    let source = NodeId::new(raw.source).map_err(corrupt_graph("edge source ID"))?;
    let target = NodeId::new(raw.target).map_err(corrupt_graph("edge target ID"))?;
    let kind = EdgeKind::new(raw.kind).map_err(corrupt_graph("edge kind"))?;
    let discriminator =
        EdgeDiscriminator::new(raw.discriminator).map_err(corrupt_graph("edge discriminator"))?;
    let properties: BTreeMap<String, Value> = serde_json::from_str(&raw.properties)?;
    let mut edge = GraphEdge::new(
        project,
        source,
        target,
        kind,
        Generation::new(sqlite_u64("edge generation", raw.generation)?),
    )
    .with_properties(properties);
    edge.discriminator = discriminator;
    Ok(edge)
}

fn edges_from(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Vec<GraphEdge>, StoreError> {
    edges_where(connection, project, "source_id", node)
}

fn edges_to(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Vec<GraphEdge>, StoreError> {
    edges_where(connection, project, "target_id", node)
}

fn edges_where(
    connection: &Connection,
    project: &ProjectId,
    column: &'static str,
    node: &NodeId,
) -> Result<Vec<GraphEdge>, StoreError> {
    let sql = format!(
        "SELECT {EDGE_COLUMNS} FROM edges WHERE project_id = ?1 AND {column} = ?2 \
         ORDER BY source_id COLLATE BINARY, target_id COLLATE BINARY, kind COLLATE BINARY, \
                  discriminator COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![project.as_str(), node.as_str()], raw_edge)?;
    rows.map(|row| edge_from_raw(row?)).collect()
}

fn search_nodes_page(
    connection: &Connection,
    project: &ProjectId,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<SearchHit>, StoreError> {
    if limit == 0 || query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow {
        field: "search limit",
        value: u64::MAX,
    })?;
    let offset = i64::try_from(offset).map_err(|_| StoreError::NumericOverflow {
        field: "search offset",
        value: u64::MAX,
    })?;
    let sql = format!(
        "SELECT {QUALIFIED_NODE_COLUMNS}, bm25(nodes_fts) \
         FROM nodes_fts JOIN nodes ON nodes.row_id = nodes_fts.rowid \
         WHERE nodes_fts MATCH ?1 AND nodes.project_id = ?2 \
         ORDER BY bm25(nodes_fts), nodes.qualified_name COLLATE BINARY, \
         nodes.node_id COLLATE BINARY LIMIT ?3 OFFSET ?4"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![query, project.as_str(), limit, offset], |row| {
        Ok((raw_node(row)?, row.get::<_, f64>(14)?))
    })?;
    rows.map(|row| {
        let (node, rank) = row?;
        Ok(SearchHit {
            node: node_from_raw(node)?,
            rank,
        })
    })
    .collect()
}

fn count_search_nodes(
    connection: &Connection,
    project: &ProjectId,
    query: &str,
) -> Result<u64, StoreError> {
    if query.is_empty() {
        return Ok(0);
    }
    let value = connection.query_row(
        "SELECT count(*) FROM nodes_fts \
         JOIN nodes ON nodes.row_id = nodes_fts.rowid \
         WHERE nodes_fts MATCH ?1 AND nodes.project_id = ?2",
        params![query, project.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    sqlite_u64("FTS match count", value)
}

fn counts(connection: &Connection, project: &ProjectId) -> Result<GraphCounts, StoreError> {
    Ok(GraphCounts {
        files: count_table(connection, "files", project)?,
        nodes: count_table(connection, "nodes", project)?,
        edges: count_table(connection, "edges", project)?,
    })
}

fn count_table(
    connection: &Connection,
    table: &'static str,
    project: &ProjectId,
) -> Result<u64, StoreError> {
    let sql = format!("SELECT count(*) FROM {table} WHERE project_id = ?1");
    let value =
        connection.query_row(&sql, params![project.as_str()], |row| row.get::<_, i64>(0))?;
    sqlite_u64("count", value)
}

fn sql_span(span: SourceSpan) -> Result<(i64, i64, i64, i64, i64, i64), StoreError> {
    Ok((
        sqlite_integer("span start byte", span.bytes.start)?,
        sqlite_integer("span end byte", span.bytes.end)?,
        sqlite_integer("span start row", span.start.row)?,
        sqlite_integer("span start column", span.start.column_bytes)?,
        sqlite_integer("span end row", span.end.row)?,
        sqlite_integer("span end column", span.end.column_bytes)?,
    ))
}

fn source_span_from_raw(values: [Option<i64>; 6]) -> Result<Option<SourceSpan>, StoreError> {
    let [
        start_byte,
        end_byte,
        start_row,
        start_column,
        end_row,
        end_column,
    ] = values;
    let Some(start_byte) = start_byte else {
        return Ok(None);
    };
    let (Some(end_byte), Some(start_row), Some(start_column), Some(end_row), Some(end_column)) =
        (end_byte, start_row, start_column, end_row, end_column)
    else {
        return Err(StoreError::CorruptData {
            field: "source span",
            reason: "partially NULL source span".to_owned(),
        });
    };
    let bytes = ByteSpan::new(
        sqlite_u64("span start byte", start_byte)?,
        sqlite_u64("span end byte", end_byte)?,
    )
    .map_err(corrupt_syntax("source span bytes"))?;
    SourceSpan::new(
        bytes,
        SourcePoint::new(
            sqlite_u64("span start row", start_row)?,
            sqlite_u64("span start column", start_column)?,
        ),
        SourcePoint::new(
            sqlite_u64("span end row", end_row)?,
            sqlite_u64("span end column", end_column)?,
        ),
    )
    .map(Some)
    .map_err(corrupt_syntax("source span"))
}

fn sqlite_integer(field: &'static str, value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericOverflow { field, value })
}

fn sqlite_u64(field: &'static str, value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::CorruptData {
        field,
        reason: format!("negative SQLite INTEGER {value}"),
    })
}

fn corrupt_graph(field: &'static str) -> impl FnOnce(GraphIdentityError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}

fn corrupt_syntax(field: &'static str) -> impl FnOnce(SyntaxIdentityError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}

fn corrupt_domain(field: &'static str) -> impl FnOnce(goldeneye_domain::DomainError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}
