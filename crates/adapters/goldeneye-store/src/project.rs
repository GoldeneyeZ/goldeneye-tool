use super::{
    BUSY_TIMEOUT, Connection, ConnectionSettings, FileRecord, Generation, OpenFlags, Path,
    ProjectId, ProjectRecord, QueryStore, Store, StoreError, TransactionBehavior,
    ensure_generation, params, project_generation, schema, sqlite_integer, sqlite_u64,
    upsert_file_in,
};

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
}

pub(super) fn configure_writable(
    connection: &Connection,
    in_memory: bool,
) -> Result<(), StoreError> {
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

pub(super) fn configure_read_only(connection: &Connection) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "query_only", true)?;
    connection.query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))?;
    Ok(())
}

pub(super) fn connection_settings(
    connection: &Connection,
) -> Result<ConnectionSettings, StoreError> {
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
