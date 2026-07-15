use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::types::{QueryError, SearchGraphRequest};

pub(super) fn search_fingerprint(request: &SearchGraphRequest) -> String {
    let values = [
        Some(request.project.as_str()),
        request.query.as_deref(),
        request.name_pattern.as_deref(),
        request.qualified_name_pattern.as_deref(),
        request.label.as_deref(),
        request.file_pattern.as_deref(),
        request.relationship.as_deref(),
    ];
    let mut hash = Sha256::new();
    for value in values {
        hash.update(value.unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    for value in [request.min_degree, request.max_degree] {
        hash.update([u8::from(value.is_some())]);
        hash.update(value.unwrap_or_default().to_le_bytes());
    }
    hash.update([
        u8::from(request.exclude_entry_points),
        u8::from(request.include_connected),
    ]);
    let mut fingerprint = String::with_capacity(16);
    for byte in &hash.finalize()[..8] {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    fingerprint
}

pub(super) fn page_offset(
    request: &SearchGraphRequest,
    fingerprint: &str,
) -> Result<usize, QueryError> {
    let Some(cursor) = request.page.cursor.as_deref() else {
        return Ok(request.page.offset);
    };
    if request.page.offset != 0 {
        return Err(QueryError::CursorWithOffset);
    }
    let mut parts = cursor.split(':');
    if parts.next() != Some("geq1") {
        return Err(QueryError::InvalidCursor);
    }
    if parts.next() != Some(fingerprint) {
        return Err(QueryError::CursorMismatch);
    }
    let offset = parts
        .next()
        .ok_or(QueryError::InvalidCursor)?
        .parse()
        .map_err(|_| QueryError::InvalidCursor)?;
    if parts.next().is_some() {
        return Err(QueryError::InvalidCursor);
    }
    Ok(offset)
}

pub(super) fn format_cursor(fingerprint: &str, offset: usize) -> String {
    format!("geq1:{fingerprint}:{offset}")
}
