use goldeneye_domain::SourcePoint;
use tree_sitter::{InputEdit, Point};

use crate::{EditContentRegion, EditPointKind, SyntaxError};

use super::SyntaxEdit;

pub(super) struct ValidatedEdit {
    pub(super) input_edit: InputEdit,
}

struct EditOffsets {
    start: usize,
    old_end: usize,
    new_end: usize,
}

pub(super) fn validate_edit(
    old_source: &[u8],
    new_source: &[u8],
    edit: SyntaxEdit,
) -> Result<ValidatedEdit, SyntaxError> {
    validate_edit_bounds(edit)?;
    let old_len = usize_to_source_len("old source", old_source.len())?;
    let new_len = usize_to_source_len("new source", new_source.len())?;
    validate_source_bounds(edit, old_len, new_len)?;
    let offsets = edit_offsets(edit)?;
    validate_edit_length(edit, old_len, new_len)?;
    validate_content(old_source, new_source, &offsets)?;
    validate_edit_points(old_source, new_source, edit, &offsets)?;
    Ok(ValidatedEdit {
        input_edit: InputEdit {
            start_byte: offsets.start,
            old_end_byte: offsets.old_end,
            new_end_byte: offsets.new_end,
            start_position: source_point_to_tree("start position", edit.start_position)?,
            old_end_position: source_point_to_tree("old end position", edit.old_end_position)?,
            new_end_position: source_point_to_tree("new end position", edit.new_end_position)?,
        },
    })
}

fn validate_edit_bounds(edit: SyntaxEdit) -> Result<(), SyntaxError> {
    if edit.start_byte > edit.old_end_byte || edit.start_byte > edit.new_end_byte {
        return Err(SyntaxError::InvalidEditBounds {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
        });
    }
    Ok(())
}

fn validate_source_bounds(edit: SyntaxEdit, old_len: u64, new_len: u64) -> Result<(), SyntaxError> {
    ensure_bound("start byte", edit.start_byte, old_len)?;
    ensure_bound("old end byte", edit.old_end_byte, old_len)?;
    ensure_bound("new end byte", edit.new_end_byte, new_len)
}

fn edit_offsets(edit: SyntaxEdit) -> Result<EditOffsets, SyntaxError> {
    Ok(EditOffsets {
        start: u64_to_usize("start byte", edit.start_byte)?,
        old_end: u64_to_usize("old end byte", edit.old_end_byte)?,
        new_end: u64_to_usize("new end byte", edit.new_end_byte)?,
    })
}

fn validate_edit_length(edit: SyntaxEdit, old_len: u64, new_len: u64) -> Result<(), SyntaxError> {
    let removed = edit.old_end_byte - edit.start_byte;
    let inserted = edit.new_end_byte - edit.start_byte;
    let expected_new_len = old_len
        .checked_sub(removed)
        .and_then(|length| length.checked_add(inserted))
        .ok_or(SyntaxError::EditLengthOverflow)?;
    if expected_new_len != new_len {
        return Err(SyntaxError::EditLengthMismatch {
            expected: expected_new_len,
            actual: new_len,
        });
    }
    Ok(())
}

fn validate_content(
    old_source: &[u8],
    new_source: &[u8],
    offsets: &EditOffsets,
) -> Result<(), SyntaxError> {
    if old_source[..offsets.start] != new_source[..offsets.start] {
        return Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Prefix,
        });
    }
    if old_source[offsets.old_end..] != new_source[offsets.new_end..] {
        return Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Suffix,
        });
    }
    Ok(())
}

fn validate_edit_points(
    old_source: &[u8],
    new_source: &[u8],
    edit: SyntaxEdit,
    offsets: &EditOffsets,
) -> Result<(), SyntaxError> {
    validate_point(
        EditPointKind::Start,
        edit.start_position,
        source_point_at(old_source, offsets.start)?,
    )?;
    validate_point(
        EditPointKind::OldEnd,
        edit.old_end_position,
        source_point_at(old_source, offsets.old_end)?,
    )?;
    validate_point(
        EditPointKind::NewEnd,
        edit.new_end_position,
        source_point_at(new_source, offsets.new_end)?,
    )?;
    Ok(())
}

fn usize_to_source_len(field: &'static str, value: usize) -> Result<u64, SyntaxError> {
    u64::try_from(value).map_err(|_| SyntaxError::SourceLengthOverflow { field })
}

fn ensure_bound(field: &'static str, offset: u64, source_len: u64) -> Result<(), SyntaxError> {
    if offset > source_len {
        return Err(SyntaxError::EditOffsetOutOfBounds {
            field,
            offset,
            source_len,
        });
    }
    Ok(())
}

fn u64_to_usize(field: &'static str, value: u64) -> Result<usize, SyntaxError> {
    usize::try_from(value).map_err(|_| SyntaxError::OffsetConversionOverflow { field, value })
}

fn source_point_at(source: &[u8], offset: usize) -> Result<SourcePoint, SyntaxError> {
    let prefix = &source[..offset];
    let mut row = 0_usize;
    for byte in prefix {
        if *byte == b'\n' {
            row += 1;
        }
    }
    let column = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    Ok(SourcePoint::new(
        usize_to_u64("source point row", row)?,
        usize_to_u64("source point column", column)?,
    ))
}

fn validate_point(
    point: EditPointKind,
    actual: SourcePoint,
    expected: SourcePoint,
) -> Result<(), SyntaxError> {
    if actual != expected {
        return Err(SyntaxError::EditPointMismatch {
            point,
            expected,
            actual,
        });
    }
    Ok(())
}

fn source_point_to_tree(field: &'static str, point: SourcePoint) -> Result<Point, SyntaxError> {
    Ok(Point::new(
        u64_to_usize(field, point.row)?,
        u64_to_usize(field, point.column_bytes)?,
    ))
}

fn usize_to_u64(field: &'static str, value: usize) -> Result<u64, SyntaxError> {
    u64::try_from(value).map_err(|_| SyntaxError::TreeSitterCoordinateOverflow { field })
}
