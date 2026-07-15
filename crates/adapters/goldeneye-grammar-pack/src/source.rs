use super::{
    BUFFER_SIZE, BufReader, File, GitSourceSession, PackError, Path, PathBuf, Read,
    ensure_safe_absolute_components, fs, invalid, is_reparse_or_symlink, open_rooted_directory,
    validate_opened_regular_file,
};

pub(super) enum SourceSession {
    Directory(DirectorySourceSession),
    Git(GitSourceSession),
}

impl SourceSession {
    pub(super) fn directory(source_root: &Path) -> Result<Self, PackError> {
        Ok(Self::Directory(DirectorySourceSession {
            root: source_root.to_path_buf(),
            directory: open_rooted_directory(source_root)?,
        }))
    }

    pub(super) fn git(repository: &Path, prefix: &str, commit: &str) -> Result<Self, PackError> {
        Ok(Self::Git(GitSourceSession::new(
            repository, prefix, commit,
        )?))
    }

    pub(super) fn with_asset<T>(
        &mut self,
        grammar_name: &str,
        asset: &str,
        operation: impl FnOnce(u64, PathBuf, &mut dyn Read) -> Result<T, PackError>,
    ) -> Result<T, PackError> {
        match self {
            Self::Directory(source) => source.with_asset(grammar_name, asset, operation),
            Self::Git(source) => source.with_asset(grammar_name, asset, operation),
        }
    }

    pub(super) fn finish(&mut self) -> Result<(), PackError> {
        match self {
            Self::Directory(_) => Ok(()),
            Self::Git(source) => source.finish(),
        }
    }
}

pub(super) struct DirectorySourceSession {
    root: PathBuf,
    directory: File,
}

impl DirectorySourceSession {
    pub(super) fn with_asset<T>(
        &self,
        grammar_name: &str,
        asset: &str,
        operation: impl FnOnce(u64, PathBuf, &mut dyn Read) -> Result<T, PackError>,
    ) -> Result<T, PackError> {
        let source_path = self.root.join(grammar_name).join(asset);
        let source_file = open_source_asset(
            &self.directory,
            &self.root,
            grammar_name,
            asset,
            &source_path,
        )?;
        let content_len = source_file
            .metadata()
            .map_err(|source| PackError::Io {
                path: source_path.clone(),
                source,
            })?
            .len();
        let mut reader = BufReader::with_capacity(BUFFER_SIZE, source_file);
        operation(content_len, source_path, &mut reader)
    }
}

pub(super) fn validate_sorted_unique(kind: &str, paths: &[String]) -> Result<(), PackError> {
    for pair in paths.windows(2) {
        if pair[0].as_bytes() >= pair[1].as_bytes() {
            return invalid(format!(
                "{kind} paths must be unique and sorted by UTF-8 bytes"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_hash(hash: &str) -> Result<(), PackError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return invalid("source_hash must be 64 lowercase hexadecimal characters");
    }
    Ok(())
}

pub(super) fn validate_exported_symbol(symbol: &str) -> Result<(), PackError> {
    if !symbol.starts_with("tree_sitter_") || symbol.len() == "tree_sitter_".len() {
        return invalid(format!(
            "exported symbol {symbol:?} must start with tree_sitter_"
        ));
    }
    if !symbol
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return invalid(format!(
            "exported symbol {symbol:?} is not an ASCII C identifier"
        ));
    }
    Ok(())
}

pub(super) fn validate_relative_path(path: &str) -> Result<(), PackError> {
    if path.is_empty() || path.starts_with('/') || path.ends_with('/') || path.contains('\\') {
        return invalid(format!(
            "asset path is not normalized and relative: {path:?}"
        ));
    }
    for component in path.split('/') {
        validate_component(component)?;
    }
    Ok(())
}

pub(super) fn validate_asset_path(path: &str) -> Result<(), PackError> {
    validate_relative_path(path)?;
    if path == "LICENSE"
        || Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "c" | "h" | "inc"))
    {
        return Ok(());
    }
    invalid(format!(
        "unsupported asset {path:?}; expected nested *.c/*.h/*.inc or direct LICENSE"
    ))
}

pub(super) fn validate_component(component: &str) -> Result<(), PackError> {
    if component.is_empty() || component == "." || component == ".." {
        return invalid(format!("unsafe path component {component:?}"));
    }
    if component.ends_with(['.', ' ']) {
        return invalid(format!(
            "path component has trailing dot/space: {component:?}"
        ));
    }
    if component.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
            )
    }) {
        return invalid(format!(
            "path component contains a reserved character: {component:?}"
        ));
    }
    let base = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return invalid(format!("reserved Windows path component: {component:?}"));
    }
    Ok(())
}

pub(super) fn ensure_safe_existing_directory(path: &Path) -> Result<(), PackError> {
    ensure_safe_absolute_components(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| PackError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_dir() {
        return invalid(format!(
            "path is not a regular directory: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn open_source_asset(
    source_directory: &File,
    source_root: &Path,
    grammar_name: &str,
    asset: &str,
    source_path: &Path,
) -> Result<File, PackError> {
    use cap_primitives::fs::{FollowSymlinks, OpenOptions, open, open_dir_nofollow};

    validate_component(grammar_name)?;
    validate_relative_path(asset)?;

    let mut current_path = source_root.join(grammar_name);
    let mut directory =
        open_dir_nofollow(source_directory, Path::new(grammar_name)).map_err(|source| {
            PackError::Io {
                path: current_path.clone(),
                source,
            }
        })?;
    let mut components = asset.split('/').peekable();
    while let Some(component) = components.next() {
        current_path.push(component);
        if components.peek().is_some() {
            directory = open_dir_nofollow(&directory, Path::new(component)).map_err(|source| {
                PackError::Io {
                    path: current_path.clone(),
                    source,
                }
            })?;
        } else {
            let mut options = OpenOptions::new();
            options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
            let file = open(&directory, Path::new(component), &options).map_err(|source| {
                PackError::Io {
                    path: current_path.clone(),
                    source,
                }
            })?;
            return validate_opened_regular_file(file, source_path);
        }
    }

    invalid(format!(
        "asset path has no components: {}",
        source_path.display()
    ))
}
