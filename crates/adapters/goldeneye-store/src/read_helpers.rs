use super::{
    BTreeMap, Connection, ContentHash, EDGE_COLUMNS, EdgeDiscriminator, EdgeKind, FileId,
    FileRecord, FromStr, Generation, GraphCounts, GraphEdge, GraphNode, NODE_COLUMNS, NodeId,
    NodeLabel, OptionalExtension, ProjectId, ProjectRecord, ProjectRelativePath,
    QUALIFIED_NODE_COLUMNS, QualifiedName, Row, SearchHit, StoreError, Value, corrupt_domain,
    corrupt_graph, corrupt_syntax, params, source_span_from_raw, sqlite_u64,
};

pub(super) fn get_project(
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

pub(super) fn list_projects(connection: &Connection) -> Result<Vec<ProjectRecord>, StoreError> {
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

pub(super) fn project_from_raw(raw: (String, String, i64)) -> Result<ProjectRecord, StoreError> {
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

pub(super) fn get_file(
    connection: &Connection,
    file: &FileId,
) -> Result<Option<FileRecord>, StoreError> {
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

pub(super) fn list_files(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<FileRecord>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT project_id, path, content_hash, generation, modified_ns, byte_len \
         FROM files WHERE project_id = ?1 ORDER BY path COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![project.as_str()], raw_file)?;
    rows.map(|row| file_from_raw(row?)).collect()
}

pub(super) fn list_nodes(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<GraphNode>, StoreError> {
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes WHERE project_id = ?1 \
         ORDER BY qualified_name COLLATE BINARY, node_id COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![project.as_str()], raw_node)?;
    rows.map(|row| node_from_raw(row?)).collect()
}

pub(super) fn list_edges(
    connection: &Connection,
    project: &ProjectId,
) -> Result<Vec<GraphEdge>, StoreError> {
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

pub(super) fn nodes_for_file(
    connection: &Connection,
    file: &FileId,
) -> Result<Vec<GraphNode>, StoreError> {
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE project_id = ?1 AND file_path = ?2 ORDER BY node_id COLLATE BINARY"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![file.project.as_str(), file.path.as_str()], raw_node)?;
    rows.map(|row| node_from_raw(row?)).collect()
}

pub(super) fn get_node(
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

pub(super) fn node_by_qualified_name(
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

pub(super) fn edges_from(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Vec<GraphEdge>, StoreError> {
    edges_where(connection, project, "source_id", node)
}

pub(super) fn edges_to(
    connection: &Connection,
    project: &ProjectId,
    node: &NodeId,
) -> Result<Vec<GraphEdge>, StoreError> {
    edges_where(connection, project, "target_id", node)
}

pub(super) fn edges_where(
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

pub(super) fn search_nodes_page(
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

pub(super) fn count_search_nodes(
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

pub(super) fn counts(
    connection: &Connection,
    project: &ProjectId,
) -> Result<GraphCounts, StoreError> {
    Ok(GraphCounts {
        files: count_table(connection, "files", project)?,
        nodes: count_table(connection, "nodes", project)?,
        edges: count_table(connection, "edges", project)?,
    })
}

pub(super) fn count_table(
    connection: &Connection,
    table: &'static str,
    project: &ProjectId,
) -> Result<u64, StoreError> {
    let sql = format!("SELECT count(*) FROM {table} WHERE project_id = ?1");
    let value =
        connection.query_row(&sql, params![project.as_str()], |row| row.get::<_, i64>(0))?;
    sqlite_u64("count", value)
}
