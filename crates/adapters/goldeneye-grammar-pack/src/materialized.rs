use super::{
    ASSET_HASH_DOMAIN, BTreeSet, BUFFER_SIZE, BufReader, Digest, GrammarPackLock, GrammarPackState,
    GrammarRecord, LOCK_HASH_DOMAIN, NATIVE_SUPPORT_HASH_DOMAIN, NativeSupportRecord, OpenOptions,
    PACK_STATE_FILE, PackError, Path, PathBuf, Read, Sha256, SourceSession, VerifiedPack, Write,
    fs, hex_digest, invalid, is_reparse_or_symlink, open_regular_file,
};

/// Hash one grammar's assets using Goldeneye's framed SHA-256 format.
///
/// # Errors
///
/// Returns [`PackError`] for unsafe paths, missing/non-regular assets, or I/O
/// failures.
pub fn hash_grammar_assets(
    source_root: impl AsRef<Path>,
    grammar: &GrammarRecord,
) -> Result<String, PackError> {
    let mut source = SourceSession::directory(source_root.as_ref())?;
    let hash = stream_grammar_assets(grammar, &mut source, None)?;
    source.finish()?;
    Ok(hash)
}

/// Hash the exact bytes of a grammar-pack lock for `pack-state.json`.
///
/// # Errors
///
/// Returns [`PackError`] when the lock is missing, unsafe, non-regular, or
/// cannot be read.
pub fn lock_file_hash(path: impl AsRef<Path>) -> Result<String, PackError> {
    let path = path.as_ref();
    let file = open_regular_file(path)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    let mut hasher = Sha256::new();
    hasher.update(LOCK_HASH_DOMAIN);
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let read = reader.read(&mut buffer).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize()))
}

pub(super) fn lock_bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(LOCK_HASH_DOMAIN);
    hasher.update(bytes);
    hex_digest(hasher.finalize())
}

/// Verify a materialized grammar pack's state, exact layout, and asset hashes.
///
/// # Errors
///
/// Returns [`PackError`] when the state file is missing, unsafe, malformed, or
/// stale; when the materialized layout differs from the lock; or when any
/// locked asset is missing, unsafe, or has drifted.
pub fn verify_materialized_pack(
    lock_path: impl AsRef<Path>,
    lock: &GrammarPackLock,
    root: impl AsRef<Path>,
) -> Result<VerifiedPack, PackError> {
    let root = root.as_ref();
    let state_path = root.join(PACK_STATE_FILE);
    let state_file = open_regular_file(&state_path)?;
    let state: GrammarPackState =
        serde_json::from_reader(BufReader::new(state_file)).map_err(|source| PackError::Json {
            path: state_path,
            source,
        })?;
    let expected = GrammarPackState::expected(lock_path, lock)?;
    if state != expected {
        return invalid("pack-state.json does not match the requested lock");
    }

    verify_materialized_layout(lock, root)?;
    lock.verify_source(root)
}

pub(super) fn verify_materialized_layout(
    lock: &GrammarPackLock,
    root: &Path,
) -> Result<(), PackError> {
    let mut expected_files = lock.locked_asset_paths().collect::<BTreeSet<_>>();
    expected_files.insert(PACK_STATE_FILE.to_owned());
    let mut expected_directories = BTreeSet::from([String::new()]);
    for file in &expected_files {
        let mut parent = Path::new(file).parent();
        while let Some(directory) = parent {
            expected_directories.insert(slash_path(directory)?);
            parent = directory.parent();
        }
    }

    let (actual_files, actual_directories) = collect_layout(root)?;
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

pub(super) fn collect_layout(
    root: &Path,
) -> Result<(BTreeSet<String>, BTreeSet<String>), PackError> {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::from([String::new()]);
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|source| PackError::Io {
                path: directory.clone(),
                source,
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| PackError::Io {
                path: directory.clone(),
                source,
            })?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| PackError::Io {
                path: path.clone(),
                source,
            })?;
            if is_reparse_or_symlink(&metadata) {
                return invalid(format!("symlink/reparse entry in pack: {}", path.display()));
            }
            let relative = slash_path(path.strip_prefix(root).map_err(|_| {
                PackError::Invalid(format!("pack path escaped root: {}", path.display()))
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

pub(super) fn slash_path(path: &Path) -> Result<String, PackError> {
    path.to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or_else(|| PackError::Invalid(format!("path is not UTF-8: {}", path.display())))
}

pub(super) fn stream_grammar_assets(
    grammar: &GrammarRecord,
    source: &mut SourceSession,
    destination_root: Option<&Path>,
) -> Result<String, PackError> {
    stream_group_assets(
        &grammar.name,
        &grammar.assets,
        ASSET_HASH_DOMAIN,
        source,
        destination_root,
    )
}

pub(super) fn stream_native_support_assets(
    support: &NativeSupportRecord,
    source: &mut SourceSession,
    destination_root: Option<&Path>,
) -> Result<String, PackError> {
    stream_group_assets(
        &support.name,
        &support.assets,
        NATIVE_SUPPORT_HASH_DOMAIN,
        source,
        destination_root,
    )
}

pub(super) fn stream_group_assets(
    group_name: &str,
    assets: &[String],
    hash_domain: &[u8],
    source: &mut SourceSession,
    destination_root: Option<&Path>,
) -> Result<String, PackError> {
    let mut hasher = Sha256::new();
    hasher.update(hash_domain);
    let mut buffer = vec![0; BUFFER_SIZE];

    for asset in assets {
        let relative = format!("{group_name}/{asset}");
        let relative_bytes = asset.as_bytes();
        source.with_asset(
            group_name,
            asset,
            |content_len, source_path, reader| {
                hasher.update((relative_bytes.len() as u64).to_be_bytes());
                hasher.update(relative_bytes);
                hasher.update(content_len.to_be_bytes());

                let mut destination = if let Some(destination_root) = destination_root {
                    let destination_path = destination_root.join(group_name).join(asset);
                    if let Some(parent) = destination_path.parent() {
                        fs::create_dir_all(parent).map_err(|source| PackError::Io {
                            path: parent.to_path_buf(),
                            source,
                        })?;
                    }
                    Some(
                        OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&destination_path)
                            .map_err(|source| PackError::Io {
                                path: destination_path,
                                source,
                            })?,
                    )
                } else {
                    None
                };

                let mut copied = 0_u64;
                loop {
                    let read = reader.read(&mut buffer).map_err(|source| PackError::Io {
                        path: source_path.clone(),
                        source,
                    })?;
                    if read == 0 {
                        break;
                    }
                    copied += read as u64;
                    hasher.update(&buffer[..read]);
                    if let Some(writer) = destination.as_mut() {
                        writer
                            .write_all(&buffer[..read])
                            .map_err(|source| PackError::Io {
                                path: PathBuf::from(&relative),
                                source,
                            })?;
                    }
                }
                if copied != content_len {
                    return invalid(format!(
                        "asset {relative} changed size while being read: expected {content_len}, got {copied}"
                    ));
                }
                if let Some(mut writer) = destination {
                    writer.flush().map_err(|source| PackError::Io {
                        path: PathBuf::from(&relative),
                        source,
                    })?;
                }
                Ok(())
            },
        )?;
    }

    Ok(hex_digest(hasher.finalize()))
}
