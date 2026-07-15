use super::{
    ByteSpan, GraphIdentityError, SourcePoint, SourceSpan, StoreError, SyntaxIdentityError,
};

pub(super) fn sql_span(span: SourceSpan) -> Result<(i64, i64, i64, i64, i64, i64), StoreError> {
    Ok((
        sqlite_integer("span start byte", span.bytes.start)?,
        sqlite_integer("span end byte", span.bytes.end)?,
        sqlite_integer("span start row", span.start.row)?,
        sqlite_integer("span start column", span.start.column_bytes)?,
        sqlite_integer("span end row", span.end.row)?,
        sqlite_integer("span end column", span.end.column_bytes)?,
    ))
}

pub(super) fn source_span_from_raw(
    values: [Option<i64>; 6],
) -> Result<Option<SourceSpan>, StoreError> {
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

pub(super) fn sqlite_integer(field: &'static str, value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericOverflow { field, value })
}

pub(super) fn sqlite_u64(field: &'static str, value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::CorruptData {
        field,
        reason: format!("negative SQLite INTEGER {value}"),
    })
}

pub(super) fn corrupt_graph(field: &'static str) -> impl FnOnce(GraphIdentityError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}

pub(super) fn corrupt_syntax(
    field: &'static str,
) -> impl FnOnce(SyntaxIdentityError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}

pub(super) fn corrupt_domain(
    field: &'static str,
) -> impl FnOnce(goldeneye_domain::DomainError) -> StoreError {
    move |error| StoreError::CorruptData {
        field,
        reason: error.to_string(),
    }
}
