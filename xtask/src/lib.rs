//! Workspace maintenance commands.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use goldeneye_syntax::{GrammarPackLock, PackError, VerifiedPack, lock_file_hash};
use serde::{Deserialize, Serialize};
use tempfile::Builder;
use thiserror::Error;

const PACK_STATE_FILE: &str = "pack-state.json";
const TEMP_MARKER_FILE: &str = ".goldeneye-owned-temp.json";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SyncOutcome {
    Created,
    AlreadyCurrent,
}

#[derive(Debug, Error)]
pub enum XtaskError {
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid grammar-pack operation: {0}")]
    Invalid(String),
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PackState {
    schema_version: u32,
    lock_hash: String,
    upstream_commit: String,
    grammar_count: usize,
    asset_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnedTempMarker {
    schema_version: u32,
    destination: String,
    lock_hash: String,
}

enum GrammarSource {
    Directory(PathBuf),
    Git { repository: PathBuf, prefix: String },
}

impl GrammarSource {
    fn directory(path: &Path) -> Result<Self, XtaskError> {
        Ok(Self::Directory(canonical_safe_directory(path)?))
    }

    fn git(repository: &Path, prefix: &str) -> Result<Self, XtaskError> {
        Ok(Self::Git {
            repository: canonical_safe_directory(repository)?,
            prefix: prefix.to_owned(),
        })
    }

    fn safety_root(&self) -> &Path {
        match self {
            Self::Directory(path) => path,
            Self::Git { repository, .. } => repository,
        }
    }

    fn verify(&self, lock: &GrammarPackLock) -> Result<VerifiedPack, PackError> {
        match self {
            Self::Directory(path) => lock.verify_source(path),
            Self::Git { repository, prefix } => lock.verify_git_source(repository, prefix),
        }
    }

    fn copy_to(
        &self,
        lock: &GrammarPackLock,
        destination: &Path,
    ) -> Result<VerifiedPack, PackError> {
        match self {
            Self::Directory(path) => lock.copy_verified_assets(path, destination),
            Self::Git { repository, prefix } => {
                lock.copy_verified_git_assets(repository, prefix, destination)
            }
        }
    }
}

/// Verify every asset referenced by a grammar-pack lock.
///
/// # Errors
///
/// Returns [`XtaskError`] when the lock, paths, assets, or hashes are invalid.
pub fn verify_grammars(
    lock_path: impl AsRef<Path>,
    source_root: impl AsRef<Path>,
) -> Result<VerifiedPack, XtaskError> {
    let lock = GrammarPackLock::load(lock_path)?;
    let source = GrammarSource::directory(source_root.as_ref())?;
    Ok(source.verify(&lock)?)
}

/// Verify every asset from the lock's exact upstream Git commit.
///
/// # Errors
///
/// Returns [`XtaskError`] when the lock, repository, prefix, pinned tree, or
/// hashes are invalid.
pub fn verify_git_grammars(
    lock_path: impl AsRef<Path>,
    git_repository: impl AsRef<Path>,
    git_prefix: &str,
) -> Result<VerifiedPack, XtaskError> {
    let lock = GrammarPackLock::load(lock_path)?;
    let source = GrammarSource::git(git_repository.as_ref(), git_prefix)?;
    Ok(source.verify(&lock)?)
}

/// Verify and atomically materialize a grammar pack, or confirm a verified no-op.
///
/// # Errors
///
/// Returns [`XtaskError`] for invalid/overlapping paths, source verification
/// failures, unsafe existing destinations, or atomic publication failures.
pub fn sync_grammars(
    lock_path: impl AsRef<Path>,
    source_root: impl AsRef<Path>,
    destination_root: impl AsRef<Path>,
) -> Result<SyncOutcome, XtaskError> {
    let source = GrammarSource::directory(source_root.as_ref())?;
    sync_grammar_source(lock_path.as_ref(), &source, destination_root.as_ref())
}

/// Verify and atomically materialize the lock's exact upstream Git tree.
///
/// # Errors
///
/// Returns [`XtaskError`] for invalid/overlapping paths, unsafe Git input,
/// source verification failures, unsafe existing destinations, or atomic
/// publication failures.
pub fn sync_git_grammars(
    lock_path: impl AsRef<Path>,
    git_repository: impl AsRef<Path>,
    git_prefix: &str,
    destination_root: impl AsRef<Path>,
) -> Result<SyncOutcome, XtaskError> {
    let source = GrammarSource::git(git_repository.as_ref(), git_prefix)?;
    sync_grammar_source(lock_path.as_ref(), &source, destination_root.as_ref())
}

fn sync_grammar_source(
    lock_path: &Path,
    source: &GrammarSource,
    destination_root: &Path,
) -> Result<SyncOutcome, XtaskError> {
    let lock = GrammarPackLock::load(lock_path)?;
    let lock_hash = lock_file_hash(lock_path)?;
    let destination = prepare_destination(destination_root)?;
    reject_overlap(source.safety_root(), &destination.path)?;

    let expected_state = PackState {
        schema_version: 1,
        lock_hash: lock_hash.clone(),
        upstream_commit: lock.upstream_commit().to_owned(),
        grammar_count: lock.grammars.len(),
        asset_count: lock.locked_asset_paths().count(),
    };

    if destination.exists {
        // A no-op remains source-driven: both the requested source and the
        // already-materialized destination are independently rehashed.
        source.verify(&lock)?;
        verify_existing_pack(&lock, &destination.path, &expected_state)?;
        return Ok(SyncOutcome::AlreadyCurrent);
    }

    let destination_name = destination
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| XtaskError::Invalid("destination must have a UTF-8 file name".into()))?;
    cleanup_owned_stale_temps(&destination.parent, destination_name)?;

    let prefix = format!(".{destination_name}.goldeneye-tmp-");
    let temporary = Builder::new()
        .prefix(&prefix)
        .tempdir_in(&destination.parent)
        .map_err(|source| XtaskError::Io {
            path: destination.parent.clone(),
            source,
        })?;
    let marker = OwnedTempMarker {
        schema_version: 1,
        destination: destination_name.to_owned(),
        lock_hash,
    };
    write_json_new(&temporary.path().join(TEMP_MARKER_FILE), &marker)?;

    let result = (|| {
        source.copy_to(&lock, temporary.path())?;
        write_json_new(&temporary.path().join(PACK_STATE_FILE), &expected_state)?;
        remove_regular_file(&temporary.path().join(TEMP_MARKER_FILE))?;
        verify_materialized_layout(&lock, temporary.path())?;
        Ok::<(), XtaskError>(())
    })();
    if let Err(error) = result {
        // TempDir owns this path, so its drop is the only cleanup authority.
        drop(temporary);
        return Err(error);
    }

    let temporary_path = temporary.keep();
    if let Err(error) = rename_no_replace(&temporary_path, &destination.path) {
        let cleanup =
            remove_just_built_temp_path(&temporary_path, &destination.parent, destination_name);
        if let Err(cleanup_error) = cleanup {
            return Err(XtaskError::Invalid(format!(
                "atomic publish failed ({error}); owned-temp cleanup also failed ({cleanup_error})"
            )));
        }
        return Err(error);
    }

    Ok(SyncOutcome::Created)
}

struct Destination {
    path: PathBuf,
    parent: PathBuf,
    exists: bool,
}

fn prepare_destination(path: &Path) -> Result<Destination, XtaskError> {
    let absolute = absolute_path(path)?;
    let name = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| XtaskError::Invalid("destination must name a UTF-8 child path".into()))?;
    validate_destination_component(name)?;

    match fs::symlink_metadata(&absolute) {
        Ok(metadata) => {
            if is_reparse_or_symlink(&metadata) || !metadata.is_dir() {
                return invalid(format!(
                    "existing destination is not a regular directory: {}",
                    absolute.display()
                ));
            }
            let canonical = canonical_safe_directory(&absolute)?;
            let parent = canonical
                .parent()
                .ok_or_else(|| XtaskError::Invalid("destination has no parent".into()))?
                .to_path_buf();
            Ok(Destination {
                path: canonical,
                parent,
                exists: true,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = absolute
                .parent()
                .ok_or_else(|| XtaskError::Invalid("destination has no parent".into()))?;
            let parent = canonical_safe_directory(parent)?;
            Ok(Destination {
                path: parent.join(name),
                parent,
                exists: false,
            })
        }
        Err(source) => Err(XtaskError::Io {
            path: absolute,
            source,
        }),
    }
}

fn verify_existing_pack(
    lock: &GrammarPackLock,
    destination: &Path,
    expected_state: &PackState,
) -> Result<(), XtaskError> {
    let state_path = destination.join(PACK_STATE_FILE);
    let state: PackState = match read_json_regular(&state_path) {
        Ok(state) => state,
        Err(error) => {
            return invalid(format!(
                "existing destination is not a verified Goldeneye pack: {error}"
            ));
        }
    };
    if state != *expected_state {
        return invalid("existing destination pack-state.json does not match the requested lock");
    }
    verify_materialized_layout(lock, destination)?;
    lock.verify_source(destination)?;
    Ok(())
}

fn verify_materialized_layout(
    lock: &GrammarPackLock,
    destination: &Path,
) -> Result<(), XtaskError> {
    let mut expected_files = lock.locked_asset_paths().collect::<BTreeSet<_>>();
    expected_files.insert(PACK_STATE_FILE.to_owned());
    let mut expected_directories = BTreeSet::from([String::new()]);
    for file in &expected_files {
        let path = Path::new(file);
        let mut parent = path.parent();
        while let Some(directory) = parent {
            let normalized = slash_path(directory)?;
            expected_directories.insert(normalized);
            parent = directory.parent();
        }
    }

    let (actual_files, actual_directories) = collect_layout(destination)?;
    if actual_files != expected_files {
        return invalid(format!(
            "materialized pack file set differs: expected {}, found {}",
            expected_files.len(),
            actual_files.len()
        ));
    }
    if actual_directories != expected_directories {
        return invalid("materialized pack contains an unexpected directory");
    }
    Ok(())
}

fn collect_layout(root: &Path) -> Result<(BTreeSet<String>, BTreeSet<String>), XtaskError> {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::from([String::new()]);
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|source| XtaskError::Io {
                path: directory.clone(),
                source,
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| XtaskError::Io {
                path: directory.clone(),
                source,
            })?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| XtaskError::Io {
                path: path.clone(),
                source,
            })?;
            if is_reparse_or_symlink(&metadata) {
                return invalid(format!("symlink/reparse entry in pack: {}", path.display()));
            }
            let relative = slash_path(path.strip_prefix(root).map_err(|_| {
                XtaskError::Invalid(format!("pack path escaped root: {}", path.display()))
            })?)?;
            if metadata.is_dir() {
                directories.insert(relative);
                stack.push(path);
            } else if metadata.is_file() {
                files.insert(relative);
            } else {
                return invalid(format!("non-regular entry in pack: {}", path.display()));
            }
        }
    }
    Ok((files, directories))
}

fn cleanup_owned_stale_temps(parent: &Path, destination_name: &str) -> Result<(), XtaskError> {
    let prefix = format!(".{destination_name}.goldeneye-tmp-");
    let mut entries = fs::read_dir(parent)
        .map_err(|source| XtaskError::Io {
            path: parent.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| XtaskError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| XtaskError::Io {
            path: path.clone(),
            source,
        })?;
        if is_reparse_or_symlink(&metadata) || !metadata.is_dir() {
            continue;
        }
        let marker: OwnedTempMarker = match read_json_regular(&path.join(TEMP_MARKER_FILE)) {
            Ok(marker) => marker,
            Err(_) => continue,
        };
        if marker.schema_version != 1
            || marker.destination != destination_name
            || !is_lower_hex_hash(&marker.lock_hash)
        {
            continue;
        }
        remove_owned_temp_path(&path, parent, destination_name)?;
    }
    Ok(())
}

fn remove_owned_temp_path(
    path: &Path,
    parent: &Path,
    destination_name: &str,
) -> Result<(), XtaskError> {
    let canonical_parent = canonical_safe_directory(parent)?;
    let canonical_path = canonical_safe_directory(path)?;
    if canonical_path.parent() != Some(canonical_parent.as_path()) {
        return invalid("owned temporary directory is not a direct destination sibling");
    }
    let name = canonical_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let prefix = format!(".{destination_name}.goldeneye-tmp-");
    if !name.starts_with(&prefix) {
        return invalid("refusing to remove a non-owned temporary directory");
    }
    let marker: OwnedTempMarker = read_json_regular(&canonical_path.join(TEMP_MARKER_FILE))?;
    if marker.schema_version != 1
        || marker.destination != destination_name
        || !is_lower_hex_hash(&marker.lock_hash)
    {
        return invalid(
            "refusing to remove a temporary directory with an invalid ownership marker",
        );
    }
    remove_just_built_temp_path(&canonical_path, &canonical_parent, destination_name)
}

fn remove_just_built_temp_path(
    path: &Path,
    parent: &Path,
    destination_name: &str,
) -> Result<(), XtaskError> {
    let canonical_parent = canonical_safe_directory(parent)?;
    let canonical_path = canonical_safe_directory(path)?;
    if canonical_path.parent() != Some(canonical_parent.as_path()) {
        return invalid("owned temporary directory is not a direct destination sibling");
    }
    let name = canonical_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let prefix = format!(".{destination_name}.goldeneye-tmp-");
    if !name.starts_with(&prefix) {
        return invalid("refusing to remove a non-owned temporary directory");
    }
    fs::remove_dir_all(&canonical_path).map_err(|source| XtaskError::Io {
        path: canonical_path,
        source,
    })?;
    Ok(())
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), XtaskError> {
    let mut bytes = serde_json::to_vec(value).map_err(|source| XtaskError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| XtaskError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&bytes).map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn read_json_regular<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, XtaskError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
        return invalid(format!("not a regular JSON file: {}", path.display()));
    }
    let mut file = File::open(path).map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| XtaskError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    serde_json::from_slice(&bytes).map_err(|source| XtaskError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn remove_regular_file(path: &Path) -> Result<(), XtaskError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
        return invalid(format!(
            "refusing to remove non-regular file: {}",
            path.display()
        ));
    }
    fs::remove_file(path).map_err(|source| XtaskError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn canonical_safe_directory(path: &Path) -> Result<PathBuf, XtaskError> {
    let absolute = absolute_path(path)?;
    reject_reparse_components(&absolute)?;
    let canonical = fs::canonicalize(&absolute).map_err(|source| XtaskError::Io {
        path: absolute,
        source,
    })?;
    let metadata = fs::symlink_metadata(&canonical).map_err(|source| XtaskError::Io {
        path: canonical.clone(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_dir() {
        return invalid(format!("not a regular directory: {}", canonical.display()));
    }
    Ok(canonical)
}

fn reject_reparse_components(path: &Path) -> Result<(), XtaskError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|source| XtaskError::Io {
            path: current.clone(),
            source,
        })?;
        if is_reparse_or_symlink(&metadata) {
            return invalid(format!(
                "symlink/reparse path component: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, XtaskError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|source| XtaskError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path))
    }
}

fn reject_overlap(source: &Path, destination: &Path) -> Result<(), XtaskError> {
    #[cfg(windows)]
    let overlap = {
        let source = windows_identity(source);
        let destination = windows_identity(destination);
        identity_contains(&source, &destination) || identity_contains(&destination, &source)
    };
    #[cfg(not(windows))]
    let overlap = source.starts_with(destination) || destination.starts_with(source);
    if overlap {
        return invalid(format!(
            "source/destination overlap rejected: {} and {}",
            source.display(),
            destination.display()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_identity(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

#[cfg(windows)]
fn identity_contains(parent: &str, child: &str) -> bool {
    child == parent
        || child
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_destination_component(component: &str) -> Result<(), XtaskError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with(['.', ' '])
        || component.chars().any(|character| {
            character.is_control() || matches!(character, ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return invalid(format!("unsafe destination component: {component:?}"));
    }
    let base = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return invalid(format!("reserved destination component: {component:?}"));
    }
    Ok(())
}

fn slash_path(path: &Path) -> Result<String, XtaskError> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| XtaskError::Invalid(format!("non-UTF-8 path: {}", path.display())))
}

fn is_lower_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(unix)]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), XtaskError> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(|source| {
        XtaskError::Io {
            path: destination.to_path_buf(),
            source: io::Error::from_raw_os_error(source.raw_os_error()),
        }
    })
}

#[cfg(not(unix))]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), XtaskError> {
    // Windows cannot replace an existing file/directory with a directory move;
    // the preflight check plus this directory rename is therefore no-clobber.
    if destination.exists() {
        return invalid(format!(
            "destination appeared before atomic publish: {}",
            destination.display()
        ));
    }
    fs::rename(source, destination).map_err(|source| XtaskError::Io {
        path: destination.to_path_buf(),
        source,
    })
}

#[cfg(windows)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn invalid<T>(message: impl Into<String>) -> Result<T, XtaskError> {
    Err(XtaskError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::remove_just_built_temp_path;

    #[test]
    fn failed_publish_cleanup_uses_in_memory_ownership_after_marker_removal() {
        let parent = tempfile::tempdir().unwrap();
        let temporary = parent.path().join(".pack.goldeneye-tmp-owned");
        fs::create_dir(&temporary).unwrap();
        fs::write(temporary.join("partially-built"), b"owned").unwrap();

        remove_just_built_temp_path(&temporary, parent.path(), "pack").unwrap();

        assert!(!temporary.exists());
    }
}
