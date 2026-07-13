//! `SQLite` persistence for Goldeneye's tool-neutral code graph.

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

pub use schema::CURRENT_SCHEMA_VERSION;

const BUSY_TIMEOUT: Duration = Duration::from_secs(10);
const NODE_COLUMNS: &str = "project_id, node_id, label, name, qualified_name, file_path, \
    start_byte, end_byte, start_row, start_column, end_row, end_column, generation, properties_json";
const QUALIFIED_NODE_COLUMNS: &str = "nodes.project_id, nodes.node_id, nodes.label, nodes.name, \
    nodes.qualified_name, nodes.file_path, nodes.start_byte, nodes.end_byte, nodes.start_row, \
    nodes.start_column, nodes.end_row, nodes.end_column, nodes.generation, nodes.properties_json";
const EDGE_COLUMNS: &str = "project_id, source_id, target_id, kind, discriminator, generation, \
    properties_json";

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
    #[error("duplicate qualified name in replacement: {0:?}")]
    DuplicateQualifiedName(QualifiedName),
    #[error("duplicate edge identity in replacement")]
    DuplicateEdge,
    #[error("stored {field} is corrupt: {reason}")]
    CorruptData { field: &'static str, reason: String },
    #[error("numeric value does not fit SQLite INTEGER: {field}={value}")]
    NumericOverflow { field: &'static str, value: u64 },
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
pub struct ReconcileOutcome {
    pub removed_files: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphCounts {
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: GraphNode,
    pub rank: f64,
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
                search_nodes(&self.connection, project, query, limit)
            }

            /// Counts normalized graph records for a project.
            ///
            /// # Errors
            ///
            /// Returns a store error when any count query fails.
            pub fn counts(&self, project: &ProjectId) -> Result<GraphCounts, StoreError> {
                counts(&self.connection, project)
            }
        }
    };
}

impl_read_api!(Store);
impl_read_api!(QueryStore);

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

fn search_nodes(
    connection: &Connection,
    project: &ProjectId,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, StoreError> {
    if limit == 0 || query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow {
        field: "search limit",
        value: u64::MAX,
    })?;
    let sql = format!(
        "SELECT {QUALIFIED_NODE_COLUMNS}, bm25(nodes_fts) \
         FROM nodes_fts JOIN nodes ON nodes.row_id = nodes_fts.rowid \
         WHERE nodes_fts MATCH ?1 AND nodes.project_id = ?2 \
         ORDER BY bm25(nodes_fts), nodes.node_id COLLATE BINARY LIMIT ?3"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![query, project.as_str(), limit], |row| {
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
