use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use goldeneye_domain::ContentHash;
use goldeneye_ports::{EditJournalRecord, EditOperationKind};

use super::{ArtifactPaths, DurableEditError};

pub(super) fn metadata(path: &Path) -> Result<fs::Metadata, DurableEditError> {
    fs::metadata(path).map_err(|source| DurableEditError::Io {
        operation: "reading metadata for",
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn read_file(path: &Path) -> Result<Vec<u8>, DurableEditError> {
    let mut file = File::open(path).map_err(|source| DurableEditError::Io {
        operation: "opening",
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| DurableEditError::Io {
            operation: "reading",
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

pub(super) fn write_temp(
    path: &Path,
    bytes: &[u8],
    permissions: Option<Permissions>,
) -> Result<(), DurableEditError> {
    let mut file = open_temp(path, permissions)?;
    file.write_all(bytes)
        .map_err(|source| DurableEditError::Io {
            operation: "writing temporary file",
            path: path.to_path_buf(),
            source,
        })?;
    file.flush().map_err(|source| DurableEditError::Io {
        operation: "flushing temporary file",
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| DurableEditError::Io {
        operation: "synchronizing temporary file",
        path: path.to_path_buf(),
        source,
    })
}

fn open_temp(path: &Path, permissions: Option<Permissions>) -> Result<File, DurableEditError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| DurableEditError::Io {
            operation: "creating temporary file",
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)
            .map_err(|source| DurableEditError::Io {
                operation: "copying permissions to",
                path: path.to_path_buf(),
                source,
            })?;
    }
    Ok(file)
}

pub(super) fn ensure_file_hash(path: &Path, expected: ContentHash) -> Result<(), DurableEditError> {
    let actual = ContentHash::of(read_file(path)?);
    if actual != expected {
        return Err(DurableEditError::StaleSource { expected, actual });
    }
    Ok(())
}

pub(super) fn hash_if_file(path: &Path) -> Result<Option<ContentHash>, DurableEditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            Ok(Some(ContentHash::of(read_file(path)?)))
        }
        Ok(_) => Err(DurableEditError::Io {
            operation: "hashing non-file",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path is not a regular file"),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DurableEditError::Io {
            operation: "inspecting",
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn path_present(path: &Path) -> Result<bool, DurableEditError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(DurableEditError::Io {
            operation: "inspecting",
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn rename_new(source_path: &Path, destination: &Path) -> Result<(), DurableEditError> {
    fs::hard_link(source_path, destination).map_err(|source| DurableEditError::Io {
        operation: "atomically linking recovery file to",
        path: destination.to_path_buf(),
        source,
    })?;
    fs::remove_file(source_path).map_err(|source| DurableEditError::Io {
        operation: "removing prior recovery link",
        path: source_path.to_path_buf(),
        source,
    })
}

pub(super) fn hard_link_new(
    source_path: &Path,
    destination: &Path,
) -> Result<(), DurableEditError> {
    fs::hard_link(source_path, destination).map_err(|source| DurableEditError::Io {
        operation: "creating no-overwrite destination",
        path: destination.to_path_buf(),
        source,
    })
}

pub(super) fn remove_if_exists(path: &Path) -> Result<(), DurableEditError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DurableEditError::Io {
            operation: "removing recovery file",
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(unix)]
pub(super) fn sync_parent(path: &Path) -> Result<(), DurableEditError> {
    let parent = path.parent().ok_or_else(|| DurableEditError::Io {
        operation: "locating parent for",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| DurableEditError::Io {
            operation: "synchronizing parent of",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(windows)]
pub(super) fn sync_parent(path: &Path) -> Result<(), DurableEditError> {
    let parent = path.parent().ok_or_else(|| DurableEditError::Io {
        operation: "locating parent for",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    match sync_windows_directory(parent) {
        Ok(()) => Ok(()),
        // Windows FlushFileBuffers rejects directory handles. File payloads and SQLite remain
        // synchronously flushed; rename-directory durability relies on NTFS journaling here.
        Err(source)
            if matches!(
                source.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(())
        }
        Err(source) => Err(DurableEditError::Io {
            operation: "synchronizing parent of",
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(windows)]
fn sync_windows_directory(parent: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent)
        .and_then(|directory| directory.sync_all())
}

pub(super) fn cleanup_artifacts(artifacts: &ArtifactPaths) -> Result<(), DurableEditError> {
    remove_if_exists(&artifacts.temp_absolute)?;
    remove_if_exists(&artifacts.backup_absolute)
}

pub(super) fn validate_journal_artifacts(
    record: &EditJournalRecord,
    artifacts: &ArtifactPaths,
) -> Result<(), DurableEditError> {
    let temp_matches = record.temp_path.as_ref() == Some(&artifacts.temp_relative);
    let backup_matches = match record.operation_kind {
        EditOperationKind::Create => record.backup_path.is_none(),
        EditOperationKind::Update | EditOperationKind::Delete => {
            record.backup_path.as_ref() == Some(&artifacts.backup_relative)
        }
    };
    if !temp_matches || !backup_matches {
        return Err(DurableEditError::JournalPathMismatch(
            record.operation_id.as_str().to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn remove_empty_confined_directory(
    project_root: &Path,
    path: &Path,
) -> Result<(), DurableEditError> {
    let Some(canonical) = confined_directory(project_root, path)? else {
        return Ok(());
    };
    remove_if_empty(canonical)
}

fn confined_directory(
    project_root: &Path,
    path: &Path,
) -> Result<Option<PathBuf>, DurableEditError> {
    if !path_present(path)? || path == project_root {
        return Ok(None);
    }
    let canonical = fs::canonicalize(path).map_err(|source| DurableEditError::Io {
        operation: "canonicalizing recovery directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(project_root) {
        return Err(DurableEditError::Io {
            operation: "validating recovery directory",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "directory escaped project"),
        });
    }
    Ok(Some(canonical))
}

fn remove_if_empty(path: PathBuf) -> Result<(), DurableEditError> {
    let mut entries = fs::read_dir(&path).map_err(|source| DurableEditError::Io {
        operation: "reading recovery directory",
        path: path.clone(),
        source,
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|source| DurableEditError::Io {
            operation: "reading recovery directory",
            path: path.clone(),
            source,
        })?
        .is_some()
    {
        return Ok(());
    }
    fs::remove_dir(&path).map_err(|source| DurableEditError::Io {
        operation: "removing empty recovery directory",
        path,
        source,
    })
}
