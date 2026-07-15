use super::{
    BTreeSet, Connection, FileId, FileRecord, Generation, GraphEdge, GraphNode, INSERT_EDGE_SQL,
    INSERT_NODE_SQL, NodeId, OptionalExtension, ProjectId, ProjectRelativePath, Statement,
    StoreError, Transaction, UPSERT_FILE_SQL, corrupt_syntax, get_project, params, sql_span,
    sqlite_integer, sqlite_u64,
};

pub(super) fn project_generation(
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

pub(super) fn ensure_generation(
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

pub(super) fn upsert_file_in(
    transaction: &Transaction<'_>,
    file: &FileRecord,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(UPSERT_FILE_SQL)?;
    upsert_file_with(&mut statement, file)
}

pub(super) fn upsert_file_with(
    statement: &mut Statement<'_>,
    file: &FileRecord,
) -> Result<(), StoreError> {
    statement.execute(params![
        file.id.project.as_str(),
        file.id.path.as_str(),
        file.content_hash.to_string(),
        sqlite_integer("file generation", file.generation.value())?,
        sqlite_integer("file modified_ns", file.modified_ns)?,
        sqlite_integer("file byte_len", file.byte_len)?,
    ])?;
    Ok(())
}

pub(super) fn validate_replacement(
    file: &FileRecord,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    validate_replacement_nodes(file, nodes)?;
    validate_replacement_edges(file, edges)
}

fn validate_replacement_nodes(file: &FileRecord, nodes: &[GraphNode]) -> Result<(), StoreError> {
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
    Ok(())
}

fn validate_replacement_edges(file: &FileRecord, edges: &[GraphEdge]) -> Result<(), StoreError> {
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

pub(super) fn validate_project_replacement(
    project: &ProjectId,
    files: &[FileRecord],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
    let file_paths = validate_project_files(project, files)?;
    let node_ids = validate_project_nodes(project, &file_paths, nodes)?;
    validate_project_edges(project, &node_ids, edges)
}

fn validate_project_files(
    project: &ProjectId,
    files: &[FileRecord],
) -> Result<BTreeSet<ProjectRelativePath>, StoreError> {
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
    Ok(file_paths)
}

fn validate_project_nodes(
    project: &ProjectId,
    file_paths: &BTreeSet<ProjectRelativePath>,
    nodes: &[GraphNode],
) -> Result<BTreeSet<NodeId>, StoreError> {
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
    Ok(node_ids)
}

fn validate_project_edges(
    project: &ProjectId,
    node_ids: &BTreeSet<NodeId>,
    edges: &[GraphEdge],
) -> Result<(), StoreError> {
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

pub(super) fn insert_node(
    transaction: &Transaction<'_>,
    node: &GraphNode,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(INSERT_NODE_SQL)?;
    insert_node_with(&mut statement, node)
}

pub(super) fn insert_node_with(
    statement: &mut Statement<'_>,
    node: &GraphNode,
) -> Result<(), StoreError> {
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
    statement.execute(params![
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
    ])?;
    Ok(())
}

pub(super) fn ensure_node_exists(
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

pub(super) fn insert_edge(
    transaction: &Transaction<'_>,
    edge: &GraphEdge,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(INSERT_EDGE_SQL)?;
    insert_edge_with(&mut statement, edge)
}

pub(super) fn insert_edge_with(
    statement: &mut Statement<'_>,
    edge: &GraphEdge,
) -> Result<(), StoreError> {
    statement.execute(params![
        edge.project.as_str(),
        edge.source.as_str(),
        edge.target.as_str(),
        edge.kind.as_str(),
        edge.discriminator.as_str(),
        sqlite_integer("edge generation", edge.generation.value())?,
        serde_json::to_string(&edge.properties)?,
    ])?;
    Ok(())
}

pub(super) fn project_file_paths(
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

pub(super) fn ensure_project_exists(
    connection: &Connection,
    project: &ProjectId,
) -> Result<(), StoreError> {
    if get_project(connection, project)?.is_none() {
        return Err(StoreError::ProjectNotFound(project.clone()));
    }
    Ok(())
}
