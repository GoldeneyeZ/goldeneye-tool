use super::{
    BTreeSet, Connection, ContentHash, EDIT_JOURNAL_COLUMNS, EditJournalRecord, EditOperationId,
    EditOperationKind, EditPhase, FromStr, NewEditJournalRecord, OptionalExtension, ProjectId,
    ProjectRelativePath, Row, Store, StoreError, TransactionBehavior, corrupt_domain,
    corrupt_syntax, params, project_generation, sqlite_u64,
};

impl Store {
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
}

pub(super) fn validate_new_edit_record(record: &NewEditJournalRecord) -> Result<(), StoreError> {
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

pub(super) fn get_edit_operation(
    connection: &Connection,
    operation_id: &EditOperationId,
) -> Result<Option<EditJournalRecord>, StoreError> {
    let sql = format!("SELECT {EDIT_JOURNAL_COLUMNS} FROM edit_journal WHERE operation_id = ?1");
    let raw = connection
        .query_row(&sql, params![operation_id.as_str()], raw_edit_operation)
        .optional()?;
    raw.map(edit_operation_from_raw).transpose()
}

pub(super) fn list_incomplete_edit_operations(
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

pub(super) fn stored_optional_hash(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<ContentHash>, StoreError> {
    value
        .map(|hash| ContentHash::from_str(&hash).map_err(corrupt_syntax(field)))
        .transpose()
}

pub(super) fn stored_optional_path(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<ProjectRelativePath>, StoreError> {
    value
        .map(|path| ProjectRelativePath::new(path).map_err(corrupt_syntax(field)))
        .transpose()
}
