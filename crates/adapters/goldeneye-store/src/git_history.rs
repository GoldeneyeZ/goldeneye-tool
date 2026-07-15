use super::{
    BTreeSet, Connection, Generation, GitCoChangeRecord, GitFileHistoryRecord, GitHistoryOutcome,
    OptionalExtension, ProjectId, ProjectRelativePath, Row, Store, StoreError, Transaction,
    TransactionBehavior, corrupt_syntax, ensure_project_exists, params, sqlite_integer, sqlite_u64,
};

impl Store {
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
        reset_git_snapshot(&transaction, project)?;
        let enriched_files = install_git_files(&transaction, project, files)?;
        let enriched_edges = install_git_couplings(&transaction, project, generation, couplings)?;
        transaction.commit()?;
        Ok(GitHistoryOutcome {
            files: files.len(),
            couplings: couplings.len(),
            enriched_files,
            enriched_edges,
        })
    }
}

fn reset_git_snapshot(
    transaction: &Transaction<'_>,
    project: &ProjectId,
) -> Result<(), StoreError> {
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
    Ok(())
}

fn install_git_files(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    files: &[GitFileHistoryRecord],
) -> Result<usize, StoreError> {
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
    Ok(enriched_files)
}

fn install_git_couplings(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    generation: Generation,
    couplings: &[GitCoChangeRecord],
) -> Result<usize, StoreError> {
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
        enriched_edges += enrich_git_coupling(transaction, project, generation, coupling)?;
    }
    Ok(enriched_edges)
}

fn enrich_git_coupling(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    generation: Generation,
    coupling: &GitCoChangeRecord,
) -> Result<usize, StoreError> {
    let source = file_node_id(transaction, project, &coupling.file_a)?;
    let target = file_node_id(transaction, project, &coupling.file_b)?;
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
        return Ok(1);
    }
    Ok(0)
}

pub(super) fn list_git_file_history(
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

pub(super) fn list_git_cochanges(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<GitCoChangeRecord>, StoreError> {
    git_cochanges_where(connection, project, None)
}

pub(super) fn coupled_files(
    connection: &Connection,
    project: &ProjectId,
    path: &ProjectRelativePath,
) -> Result<Vec<GitCoChangeRecord>, StoreError> {
    git_cochanges_where(connection, project, Some(path))
}

pub(super) fn git_cochanges_where(
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

pub(super) fn validate_git_history(
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

pub(super) fn file_node_id(
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
