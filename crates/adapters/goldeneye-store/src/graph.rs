use super::{
    BTreeSet, FileId, FileRecord, Generation, GraphEdge, GraphNode, INSERT_EDGE_SQL,
    INSERT_NODE_SQL, ProjectId, ProjectRecord, ProjectRelativePath, ProjectReplacementOutcome,
    ReconcileOutcome, ReplacementOutcome, Store, StoreError, Transaction, TransactionBehavior,
    UPSERT_FILE_SQL, ensure_generation, ensure_node_exists, insert_edge, insert_edge_with,
    insert_node, insert_node_with, params, project_file_paths, project_generation, sqlite_integer,
    upsert_file_in, upsert_file_with, validate_project_replacement, validate_replacement,
};

impl Store {
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
        let (ordered_nodes, ordered_edges) = ordered_file_graph(nodes, edges);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        replace_file_graph_in(&transaction, file, &ordered_nodes, &ordered_edges)?;
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
        order_project_graph(&mut files, &mut nodes, &mut edges);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = register_project_generation(&transaction, project)?;
        assign_generation(generation, &mut files, &mut nodes, &mut edges);
        replace_project_rows(&transaction, &project.id, &files, &nodes, &edges)?;

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
        validate_retained_paths(project, retained, &existing)?;
        let removed_files =
            reconcile_project_rows(&transaction, project, generation_sql, retained, &existing)?;
        transaction.commit()?;
        Ok(ReconcileOutcome { removed_files })
    }
}

fn ordered_file_graph(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut ordered_nodes = nodes.to_vec();
    ordered_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    let mut ordered_edges = edges.to_vec();
    sort_edges(&mut ordered_edges);
    (ordered_nodes, ordered_edges)
}

fn replace_file_graph_in(
    transaction: &Transaction<'_>,
    file: &FileRecord,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    ensure_generation(transaction, &file.id.project, file.generation)?;
    transaction.execute(
        "DELETE FROM nodes WHERE project_id = ?1 AND file_path = ?2",
        params![file.id.project.as_str(), file.id.path.as_str()],
    )?;
    upsert_file_in(transaction, file)?;
    for node in nodes {
        insert_node(transaction, node)?;
    }
    for edge in edges {
        ensure_node_exists(transaction, &edge.project, &edge.source)?;
        ensure_node_exists(transaction, &edge.project, &edge.target)?;
        insert_edge(transaction, edge)?;
    }
    Ok(())
}

fn order_project_graph(files: &mut [FileRecord], nodes: &mut [GraphNode], edges: &mut [GraphEdge]) {
    files.sort_by(|left, right| left.id.path.cmp(&right.id.path));
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    sort_edges(edges);
}

fn sort_edges(edges: &mut [GraphEdge]) {
    edges.sort_by(|left, right| {
        (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
            &right.source,
            &right.target,
            &right.kind,
            &right.discriminator,
        ))
    });
}

fn register_project_generation(
    transaction: &Transaction<'_>,
    project: &ProjectRecord,
) -> Result<Generation, StoreError> {
    let initial_generation = sqlite_integer("project generation", project.generation.value())?;
    transaction.execute(
        "INSERT INTO projects(id, root_path, current_generation) VALUES (?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
        params![project.id.as_str(), project.root_path, initial_generation],
    )?;
    let current = project_generation(transaction, &project.id)?;
    let next_value = current
        .value()
        .checked_add(1)
        .ok_or_else(|| StoreError::GenerationOverflow(project.id.clone()))?;
    transaction.execute(
        "UPDATE projects SET current_generation = ?2 WHERE id = ?1",
        params![
            project.id.as_str(),
            sqlite_integer("project generation", next_value)?
        ],
    )?;
    Ok(Generation::new(next_value))
}

fn assign_generation(
    generation: Generation,
    files: &mut [FileRecord],
    nodes: &mut [GraphNode],
    edges: &mut [GraphEdge],
) {
    for file in files {
        file.generation = generation;
    }
    for node in nodes {
        node.generation = generation;
    }
    for edge in edges {
        edge.generation = generation;
    }
}

fn replace_project_rows(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    files: &[FileRecord],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM nodes WHERE project_id = ?1",
        params![project.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM files WHERE project_id = ?1",
        params![project.as_str()],
    )?;
    let mut statement = transaction.prepare(UPSERT_FILE_SQL)?;
    for file in files {
        upsert_file_with(&mut statement, file)?;
    }
    drop(statement);
    let mut statement = transaction.prepare(INSERT_NODE_SQL)?;
    for node in nodes {
        insert_node_with(&mut statement, node)?;
    }
    drop(statement);
    let mut statement = transaction.prepare(INSERT_EDGE_SQL)?;
    for edge in edges {
        insert_edge_with(&mut statement, edge)?;
    }
    Ok(())
}

fn validate_retained_paths(
    project: &ProjectId,
    retained: &BTreeSet<ProjectRelativePath>,
    existing: &BTreeSet<ProjectRelativePath>,
) -> Result<(), StoreError> {
    for path in retained {
        if !existing.contains(path) {
            return Err(StoreError::FileNotFound(FileId::new(
                project.clone(),
                path.clone(),
            )));
        }
    }
    Ok(())
}

fn reconcile_project_rows(
    transaction: &Transaction<'_>,
    project: &ProjectId,
    generation_sql: i64,
    retained: &BTreeSet<ProjectRelativePath>,
    existing: &BTreeSet<ProjectRelativePath>,
) -> Result<usize, StoreError> {
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
    Ok(removed_files)
}
