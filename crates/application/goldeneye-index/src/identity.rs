use std::fs;
use std::path::{Path, PathBuf};

use goldeneye_domain::{ProjectId, ProjectRecord};

use crate::IndexError;

/// Returns an upstream-compatible canonical root string.
///
/// # Errors
///
/// Returns an I/O error when the path is absent or cannot be made absolute, or a UTF-8 error when
/// the platform path cannot be represented losslessly.
pub fn canonical_root_string(root: impl AsRef<Path>) -> Result<String, IndexError> {
    let canonical = canonical_existing_path(root.as_ref()).map_err(|source| IndexError::Io {
        path: root.as_ref().to_path_buf(),
        source,
    })?;
    normalized_root(&canonical)
}

/// Derives the pinned upstream project ID from an existing repository root.
///
/// # Errors
///
/// Returns an I/O, UTF-8, or domain identity error when canonicalization or validation fails.
pub fn project_id_for_root(root: impl AsRef<Path>) -> Result<ProjectId, IndexError> {
    let root = canonical_root_string(root)?;
    project_id_from_normalized_root(&root)
}

/// Sanitizes an explicit project-name override using the pinned upstream mapping.
///
/// # Errors
///
/// Returns a domain identity error when the sanitized value is invalid.
pub fn project_id_for_name(name: &str) -> Result<ProjectId, IndexError> {
    project_id_from_normalized_root(name)
}

/// Builds a generation-zero project record for an existing repository root.
///
/// # Errors
///
/// Returns an I/O, UTF-8, domain identity, or graph identity error when the root is invalid.
pub fn canonical_project(root: impl AsRef<Path>) -> Result<ProjectRecord, IndexError> {
    let root_path = canonical_root_string(root)?;
    let id = project_id_from_normalized_root(&root_path)?;
    Ok(ProjectRecord::new(id, root_path)?)
}

fn normalized_root(root: &Path) -> Result<String, IndexError> {
    let raw = root
        .to_str()
        .ok_or_else(|| IndexError::NonUtf8Root(PathBuf::from(root)))?;
    let normalized = raw.replace('\\', "/");
    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    if normalized == "/"
        || (normalized.len() == 3
            && normalized.as_bytes()[1] == b':'
            && normalized.as_bytes()[2] == b'/')
    {
        Ok(normalized.to_owned())
    } else {
        Ok(normalized.trim_end_matches('/').to_owned())
    }
}

fn project_id_from_normalized_root(root: &str) -> Result<ProjectId, IndexError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut mapped = String::with_capacity(root.len());
    for byte in root.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            mapped.push(char::from(byte));
        } else if byte >= 0x80 {
            mapped.push(char::from(HEX[usize::from(byte >> 4)]));
            mapped.push(char::from(HEX[usize::from(byte & 0x0f)]));
        } else {
            mapped.push('-');
        }
    }
    let mut collapsed = String::with_capacity(mapped.len());
    for byte in mapped.bytes() {
        if matches!(byte, b'-' | b'.') && collapsed.as_bytes().last() == Some(&byte) {
            continue;
        }
        collapsed.push(char::from(byte));
    }
    let trimmed = collapsed
        .trim_start_matches(['-', '.'])
        .trim_end_matches('-');
    if trimmed.is_empty() {
        return Ok(ProjectId::new("root")?);
    }
    let mut id = trimmed.to_owned();
    if id.len() > 200 {
        let hash = id.bytes().fold(2_166_136_261_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
        });
        id.truncate(191);
        id.push('-');
        for shift in (0..8).rev() {
            let nibble = ((hash >> (shift * 4)) & 0x0f) as usize;
            id.push(char::from(HEX[nibble]));
        }
    }
    Ok(ProjectId::new(id)?)
}

#[cfg(windows)]
fn canonical_existing_path(root: &Path) -> std::io::Result<PathBuf> {
    fs::metadata(root)?;
    std::path::absolute(root)
}

#[cfg(not(windows))]
fn canonical_existing_path(root: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(root)
}

#[cfg(test)]
mod tests {
    use super::project_id_from_normalized_root;

    #[test]
    fn pinned_project_id_sanitizer_handles_windows_unicode_and_root_paths() {
        assert_eq!(
            project_id_from_normalized_root("C:/Users/dev/project")
                .expect("Windows path")
                .as_str(),
            "C-Users-dev-project"
        );
        assert_eq!(
            project_id_from_normalized_root("/Users/yunxin/Desktop/开发/后端/信租风控通后端")
                .expect("Unicode path")
                .as_str(),
            "Users-yunxin-Desktop-e5bc80e58f91-e5908ee7abaf-e4bfa1e7a79fe9a38ee68ea7e9809ae5908ee7abaf"
        );
        assert_eq!(
            project_id_from_normalized_root("///")
                .expect("root path")
                .as_str(),
            "root"
        );
        assert_eq!(
            project_id_from_normalized_root(".hidden//name__part")
                .expect("safe punctuation")
                .as_str(),
            "hidden-name__part"
        );
    }

    #[test]
    fn pinned_project_id_sanitizer_caps_with_full_fnv1a_hash() {
        let value = "a".repeat(201);
        let id = project_id_from_normalized_root(&value).expect("long ID");
        assert_eq!(id.as_str().len(), 200);
        assert!(id.as_str().ends_with("-876056a4"));
    }

    #[test]
    fn normalized_roots_keep_posix_and_windows_drive_roots_addressable() {
        assert_eq!(
            super::normalized_root(std::path::Path::new("/")).expect("POSIX root"),
            "/"
        );
        assert_eq!(
            super::normalized_root(std::path::Path::new("C:/")).expect("drive root"),
            "C:/"
        );
        assert_eq!(
            super::normalized_root(std::path::Path::new("/tmp/repo///"))
                .expect("trailing separators"),
            "/tmp/repo"
        );
    }
}
