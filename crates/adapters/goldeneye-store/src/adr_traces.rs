use super::{
    ADR_MAX_LENGTH, ADR_MAX_SECTIONS, AdrRecord, AdrSection, BTreeMap, Connection,
    OptionalExtension, ProjectId, RuntimeTrace, RuntimeTraceRecord, Store, StoreError,
    TransactionBehavior, corrupt_domain, ensure_project_exists, params, parse_adr_sections,
    render_adr_sections, sqlite_integer, sqlite_u64,
};

impl Store {
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
}

pub(super) fn get_adr(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Option<AdrRecord>, StoreError> {
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

pub(super) fn list_runtime_traces(
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

pub(super) fn validate_runtime_trace(trace: &RuntimeTrace) -> Result<(), StoreError> {
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
