use super::{
    ArtifactError, Connection, Digest, EXPLICIT_INDEXES, OpenFlags, Path, PathBuf, Sha256,
    SystemTime, UNIX_EPOCH,
};

pub(super) fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    path.with_file_name(format!(".{name}.{suffix}.{}.{}", std::process::id(), nonce))
}

pub(super) fn strip_indexes(path: &Path) -> Result<(), ArtifactError> {
    let connection = Connection::open(path)?;
    for index in EXPLICIT_INDEXES {
        connection.execute_batch(&format!("DROP INDEX IF EXISTS \"{index}\";"))?;
    }
    connection.execute_batch("VACUUM;")?;
    Ok(())
}

pub(super) fn verify_database(path: &Path) -> Result<(), ArtifactError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(ArtifactError::Integrity(result));
    }
    let projects = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='projects'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let projects = nonnegative_count(projects, "projects table")?;
    if projects != 1 {
        return Err(ArtifactError::Integrity("missing projects table".into()));
    }
    Ok(())
}

pub(super) fn project_counts(path: &Path, project: &str) -> Result<(u64, u64), ArtifactError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let nodes = connection.query_row(
        "SELECT count(*) FROM nodes WHERE project_id = ?1",
        [project],
        |row| row.get::<_, i64>(0),
    )?;
    let edges = connection.query_row(
        "SELECT count(*) FROM edges WHERE project_id = ?1",
        [project],
        |row| row.get::<_, i64>(0),
    )?;
    Ok((
        nonnegative_count(nodes, "node")?,
        nonnegative_count(edges, "edge")?,
    ))
}

pub(super) fn nonnegative_count(value: i64, subject: &str) -> Result<u64, ArtifactError> {
    u64::try_from(value)
        .map_err(|_| ArtifactError::Integrity(format!("negative {subject} count: {value}")))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
