use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const ASSET_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-assets-v1\0";
const LOCK_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-lock-v1\0";
const BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum PackError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid grammar lock TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid grammar lock: {0}")]
    Invalid(String),
    #[error("grammar asset hash mismatch for {grammar}: expected {expected}, got {actual}")]
    HashMismatch {
        grammar: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarPackLock {
    schema_version: u32,
    upstream_repository: String,
    upstream_commit: String,
    declared_grammar_count: usize,
    declared_language_binding_count: usize,
    compatible_abi_min: u32,
    compatible_abi_max: u32,
    hash_algorithm: String,
    hash_domain: String,
    pub grammars: Vec<GrammarRecord>,
    pub language_mappings: Vec<LanguageMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarRecord {
    pub name: String,
    pub repository: String,
    pub commit: Option<String>,
    pub missing_commit_reason: Option<String>,
    pub abi: u32,
    pub assets: Vec<String>,
    pub source_hash: String,
    pub scanner_language: String,
    pub license_files: Vec<String>,
    pub verdict: String,
    #[serde(default)]
    pub provenance_notes: Vec<String>,
    pub orphan_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageBindingStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageMapping {
    pub language_id: String,
    pub status: LanguageBindingStatus,
    pub grammar: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct VerifiedPack {
    pub grammar_count: usize,
    pub asset_count: usize,
}

impl GrammarPackLock {
    /// Load and validate a grammar-pack lock from TOML.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when the file cannot be read, TOML cannot be
    /// decoded, or any lock invariant fails.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PackError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&source)
    }

    /// Parse and validate a grammar-pack lock from TOML text.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when TOML decoding or lock validation fails.
    pub fn parse(source: &str) -> Result<Self, PackError> {
        let lock: Self = toml::from_str(source)?;
        lock.validate()?;
        Ok(lock)
    }

    #[must_use]
    pub fn upstream_commit(&self) -> &str {
        &self.upstream_commit
    }

    #[must_use]
    pub fn upstream_repository(&self) -> &str {
        &self.upstream_repository
    }

    #[must_use]
    pub fn abi_histogram(&self) -> BTreeMap<u32, usize> {
        let mut histogram = BTreeMap::new();
        for grammar in &self.grammars {
            *histogram.entry(grammar.abi).or_insert(0) += 1;
        }
        histogram
    }

    #[must_use]
    pub fn available_language_count(&self) -> usize {
        self.language_mappings
            .iter()
            .filter(|mapping| mapping.status == LanguageBindingStatus::Available)
            .count()
    }

    #[must_use]
    pub fn unique_bound_grammar_count(&self) -> usize {
        self.language_mappings
            .iter()
            .filter_map(|mapping| mapping.grammar.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    }

    #[must_use]
    pub fn unavailable_language_ids(&self) -> Vec<&str> {
        self.language_mappings
            .iter()
            .filter(|mapping| mapping.status == LanguageBindingStatus::Unavailable)
            .map(|mapping| mapping.language_id.as_str())
            .collect()
    }

    #[must_use]
    pub fn orphan_grammar_names(&self) -> Vec<&str> {
        self.grammars
            .iter()
            .filter(|grammar| grammar.orphan_reason.is_some())
            .map(|grammar| grammar.name.as_str())
            .collect()
    }

    #[must_use]
    pub fn grammar_name_for(&self, language_id: &str) -> Option<&str> {
        self.language_mappings
            .iter()
            .find(|mapping| mapping.language_id == language_id)
            .and_then(|mapping| mapping.grammar.as_deref())
    }

    pub fn locked_asset_paths(&self) -> impl Iterator<Item = String> + '_ {
        self.grammars.iter().flat_map(|grammar| {
            grammar
                .assets
                .iter()
                .map(move |asset| format!("{}/{asset}", grammar.name))
        })
    }

    /// Verify every locked source asset and grammar hash.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for unsafe paths, missing/non-regular assets,
    /// I/O failures, or content-hash mismatches.
    pub fn verify_source(&self, source_root: impl AsRef<Path>) -> Result<VerifiedPack, PackError> {
        self.stream_assets(source_root.as_ref(), None)
    }

    /// Copy locked assets while hashing the same open source handles.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for unsafe paths, pre-existing destination
    /// files, I/O failures, or content-hash mismatches.
    pub fn copy_verified_assets(
        &self,
        source_root: impl AsRef<Path>,
        destination_root: impl AsRef<Path>,
    ) -> Result<VerifiedPack, PackError> {
        self.stream_assets(source_root.as_ref(), Some(destination_root.as_ref()))
    }

    fn stream_assets(
        &self,
        source_root: &Path,
        destination_root: Option<&Path>,
    ) -> Result<VerifiedPack, PackError> {
        let source_directory = open_rooted_directory(source_root)?;
        if let Some(destination_root) = destination_root {
            ensure_safe_existing_directory(destination_root)?;
        }

        let mut asset_count = 0;
        for grammar in &self.grammars {
            let actual =
                stream_grammar_assets(grammar, source_root, &source_directory, destination_root)?;
            if actual != grammar.source_hash {
                return Err(PackError::HashMismatch {
                    grammar: grammar.name.clone(),
                    expected: grammar.source_hash.clone(),
                    actual,
                });
            }
            asset_count += grammar.assets.len();
        }

        Ok(VerifiedPack {
            grammar_count: self.grammars.len(),
            asset_count,
        })
    }

    fn validate(&self) -> Result<(), PackError> {
        self.validate_header()?;
        let grammar_names = self.validate_grammars()?;
        let bound_grammars = self.validate_language_mappings(&grammar_names)?;
        self.validate_binding_states(&bound_grammars)
    }

    fn validate_header(&self) -> Result<(), PackError> {
        if self.schema_version != 1 {
            return invalid(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        require_nonempty("upstream_repository", &self.upstream_repository)?;
        require_nonempty("upstream_commit", &self.upstream_commit)?;
        if self.hash_algorithm != "sha256" {
            return invalid("hash_algorithm must be sha256");
        }
        if self.hash_domain != "goldeneye-grammar-assets-v1" {
            return invalid("hash_domain must be goldeneye-grammar-assets-v1");
        }
        if self.compatible_abi_min > self.compatible_abi_max {
            return invalid("compatible ABI range is reversed");
        }
        if self.declared_grammar_count != self.grammars.len() {
            return invalid(format!(
                "declared grammar count {} does not match {} records",
                self.declared_grammar_count,
                self.grammars.len()
            ));
        }
        if self.declared_language_binding_count != self.language_mappings.len() {
            return invalid(format!(
                "declared language-binding count {} does not match {} records",
                self.declared_language_binding_count,
                self.language_mappings.len()
            ));
        }

        Ok(())
    }

    fn validate_grammars(&self) -> Result<BTreeSet<String>, PackError> {
        let mut grammar_names = BTreeSet::new();
        let mut destination_paths = BTreeSet::new();
        for grammar in &self.grammars {
            validate_component(&grammar.name)?;
            if !grammar_names.insert(grammar.name.clone()) {
                return invalid(format!("duplicate grammar name {}", grammar.name));
            }
            require_nonempty("grammar repository", &grammar.repository)?;
            match (&grammar.commit, &grammar.missing_commit_reason) {
                (Some(commit), None) => require_nonempty("grammar commit", commit)?,
                (None, Some(reason)) => require_nonempty("missing commit reason", reason)?,
                _ => {
                    return invalid(format!(
                        "grammar {} must declare exactly one of commit or missing_commit_reason",
                        grammar.name
                    ));
                }
            }
            if !(self.compatible_abi_min..=self.compatible_abi_max).contains(&grammar.abi) {
                return invalid(format!(
                    "grammar {} ABI {} is outside {}..={}",
                    grammar.name, grammar.abi, self.compatible_abi_min, self.compatible_abi_max
                ));
            }
            require_nonempty("scanner_language", &grammar.scanner_language)?;
            require_nonempty("verdict", &grammar.verdict)?;
            validate_hash(&grammar.source_hash)?;
            validate_sorted_unique("asset", &grammar.assets)?;
            validate_sorted_unique("license", &grammar.license_files)?;
            if grammar.assets.is_empty() {
                return invalid(format!("grammar {} has no assets", grammar.name));
            }
            if grammar.license_files.is_empty() {
                return invalid(format!("grammar {} has no license files", grammar.name));
            }
            if grammar.license_files.as_slice() != ["LICENSE"] {
                return invalid(format!(
                    "grammar {} must declare exactly one direct LICENSE",
                    grammar.name
                ));
            }
            if !grammar.assets.iter().any(|asset| asset == "parser.c") {
                return invalid(format!(
                    "grammar {} must lock its direct parser.c",
                    grammar.name
                ));
            }
            let assets = grammar.assets.iter().collect::<BTreeSet<_>>();
            for asset in &grammar.assets {
                validate_asset_path(asset)?;
                let destination = format!("{}/{}", grammar.name, asset).to_lowercase();
                if !destination_paths.insert(destination) {
                    return invalid(format!(
                        "case-folded destination collision at {}/{}",
                        grammar.name, asset
                    ));
                }
            }
            for license in &grammar.license_files {
                validate_relative_path(license)?;
                if !assets.contains(license) {
                    return invalid(format!(
                        "grammar {} license {} is not a locked asset",
                        grammar.name, license
                    ));
                }
            }
            for note in &grammar.provenance_notes {
                require_nonempty("provenance note", note)?;
            }
            if let Some(reason) = &grammar.orphan_reason {
                require_nonempty("orphan reason", reason)?;
            }
        }

        Ok(grammar_names)
    }

    fn validate_language_mappings(
        &self,
        grammar_names: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>, PackError> {
        let mut language_ids = BTreeSet::new();
        let mut bound_grammars = BTreeSet::new();
        for mapping in &self.language_mappings {
            validate_component(&mapping.language_id)?;
            if !language_ids.insert(mapping.language_id.clone()) {
                return invalid(format!("duplicate language id {}", mapping.language_id));
            }
            match mapping.status {
                LanguageBindingStatus::Available => {
                    let grammar = mapping.grammar.as_deref().ok_or_else(|| {
                        PackError::Invalid(format!(
                            "available language {} has no grammar",
                            mapping.language_id
                        ))
                    })?;
                    if mapping.reason.is_some() {
                        return invalid(format!(
                            "available language {} must not have an unavailable reason",
                            mapping.language_id
                        ));
                    }
                    if !grammar_names.contains(grammar) {
                        return invalid(format!(
                            "language {} references unknown grammar {grammar}",
                            mapping.language_id
                        ));
                    }
                    bound_grammars.insert(grammar.to_owned());
                }
                LanguageBindingStatus::Unavailable => {
                    if mapping.grammar.is_some() {
                        return invalid(format!(
                            "unavailable language {} must not name a grammar",
                            mapping.language_id
                        ));
                    }
                    require_nonempty(
                        "unavailable reason",
                        mapping.reason.as_deref().unwrap_or_default(),
                    )?;
                }
            }
        }

        Ok(bound_grammars)
    }

    fn validate_binding_states(&self, bound_grammars: &BTreeSet<String>) -> Result<(), PackError> {
        for grammar in &self.grammars {
            let bound = bound_grammars.contains(grammar.name.as_str());
            if bound == grammar.orphan_reason.is_some() {
                return invalid(format!(
                    "grammar {} must be explicitly either bound or orphaned",
                    grammar.name
                ));
            }
        }

        Ok(())
    }
}

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
    let source_root = source_root.as_ref();
    let source_directory = open_rooted_directory(source_root)?;
    stream_grammar_assets(grammar, source_root, &source_directory, None)
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

fn stream_grammar_assets(
    grammar: &GrammarRecord,
    source_root: &Path,
    source_directory: &File,
    destination_root: Option<&Path>,
) -> Result<String, PackError> {
    let mut hasher = Sha256::new();
    hasher.update(ASSET_HASH_DOMAIN);
    let mut buffer = vec![0; BUFFER_SIZE];

    for asset in &grammar.assets {
        let relative = format!("{}/{}", grammar.name, asset);
        let relative_bytes = asset.as_bytes();
        let source_path = source_root.join(&grammar.name).join(asset);
        let source_file = open_source_asset(
            source_directory,
            source_root,
            &grammar.name,
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

        hasher.update((relative_bytes.len() as u64).to_be_bytes());
        hasher.update(relative_bytes);
        hasher.update(content_len.to_be_bytes());

        let mut reader = BufReader::with_capacity(BUFFER_SIZE, source_file);
        let mut destination = if let Some(destination_root) = destination_root {
            let destination_path = destination_root.join(&grammar.name).join(asset);
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|source| PackError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination_path)
                .map_err(|source| PackError::Io {
                    path: destination_path,
                    source,
                })?;
            Some(BufWriter::with_capacity(BUFFER_SIZE, file))
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
    }

    Ok(hex_digest(hasher.finalize()))
}

fn validate_sorted_unique(kind: &str, paths: &[String]) -> Result<(), PackError> {
    for pair in paths.windows(2) {
        if pair[0].as_bytes() >= pair[1].as_bytes() {
            return invalid(format!(
                "{kind} paths must be unique and sorted by UTF-8 bytes"
            ));
        }
    }
    Ok(())
}

fn validate_hash(hash: &str) -> Result<(), PackError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return invalid("source_hash must be 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), PackError> {
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

fn validate_asset_path(path: &str) -> Result<(), PackError> {
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

fn validate_component(component: &str) -> Result<(), PackError> {
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

fn ensure_safe_existing_directory(path: &Path) -> Result<(), PackError> {
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

fn open_source_asset(
    source_directory: &File,
    source_root: &Path,
    grammar_name: &str,
    asset: &str,
    source_path: &Path,
) -> Result<File, PackError> {
    use cap_primitives::fs::{FollowSymlinks, OpenOptions, open, open_dir_nofollow};

    validate_component(grammar_name)?;
    validate_asset_path(asset)?;

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

fn split_rooted_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), PackError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PackError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    if !absolute.is_absolute() {
        return invalid(format!("path is not absolute: {}", absolute.display()));
    }

    let mut anchor = PathBuf::new();
    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if components.is_empty() => {
                anchor.push(component.as_os_str());
            }
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                return invalid(format!(
                    "parent traversal rejected in path: {}",
                    path.display()
                ));
            }
            Component::Prefix(_) | Component::RootDir => {
                return invalid(format!("unexpected path root: {}", path.display()));
            }
        }
    }
    if anchor.as_os_str().is_empty() {
        return invalid(format!("path has no filesystem root: {}", path.display()));
    }
    Ok((anchor, components))
}

fn open_directory_chain(anchor: &Path, components: &[OsString]) -> Result<File, PackError> {
    use cap_primitives::fs::{open_ambient_dir, open_dir_nofollow};

    let mut current = anchor.to_path_buf();
    let mut directory =
        open_ambient_dir(anchor, cap_primitives::ambient_authority()).map_err(|source| {
            PackError::Io {
                path: current.clone(),
                source,
            }
        })?;
    for component in components {
        current.push(component);
        directory = open_dir_nofollow(&directory, Path::new(component)).map_err(|source| {
            PackError::Io {
                path: current.clone(),
                source,
            }
        })?;
    }
    Ok(directory)
}

fn open_rooted_directory(path: &Path) -> Result<File, PackError> {
    let (anchor, components) = split_rooted_path(path)?;
    open_directory_chain(&anchor, &components)
}

fn open_rooted_regular_file(path: &Path) -> Result<File, PackError> {
    use cap_primitives::fs::{FollowSymlinks, OpenOptions, open};

    let (anchor, mut components) = split_rooted_path(path)?;
    let file_name = components
        .pop()
        .ok_or_else(|| PackError::Invalid(format!("path has no file name: {}", path.display())))?;
    let directory = open_directory_chain(&anchor, &components)?;
    let mut options = OpenOptions::new();
    options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
    let file =
        open(&directory, Path::new(&file_name), &options).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    validate_opened_regular_file(file, path)
}

fn ensure_safe_absolute_components(path: &Path) -> Result<(), PackError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PackError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|source| PackError::Io {
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

fn open_regular_file(path: &Path) -> Result<File, PackError> {
    open_rooted_regular_file(path)
}

fn validate_opened_regular_file(file: File, path: &Path) -> Result<File, PackError> {
    let metadata = file.metadata().map_err(|source| PackError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
        return invalid(format!("asset is not a regular file: {}", path.display()));
    }
    Ok(file)
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

fn require_nonempty(field: &str, value: &str) -> Result<(), PackError> {
    if value.trim().is_empty() {
        return invalid(format!("{field} must not be empty"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, PackError> {
    Err(PackError::Invalid(message.into()))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}
