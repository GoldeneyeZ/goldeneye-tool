//! Workspace maintenance commands.

pub mod architecture;

pub use architecture::{ArchitectureError, ArchitectureReport, verify_architecture};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use goldeneye_grammar_pack::{
    GrammarPackLock, GrammarPackState, GrammarRecord, LanguageBindingStatus, LanguageMapping,
    PACK_STATE_FILE, PackError, VerifiedPack, verify_materialized_pack,
};
use serde::{Deserialize, Serialize};
use tempfile::Builder;
use thiserror::Error;

const TEMP_MARKER_FILE: &str = ".goldeneye-owned-temp.json";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SyncOutcome {
    Created,
    AlreadyCurrent,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GenerationOutcome {
    Written,
    Unchanged,
}

#[derive(Debug, Error)]
pub enum XtaskError {
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("existing destination is not a verified Goldeneye pack: {source}")]
    ExistingPack {
        #[source]
        source: PackError,
    },
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

/// Render the checked-in full-provider registry from one validated lock buffer.
///
/// # Errors
///
/// Returns [`XtaskError`] when the lock cannot be read, hashed, or validated,
/// or when its audited full-pack cardinalities have drifted.
pub fn render_provider(lock_path: impl AsRef<Path>) -> Result<String, XtaskError> {
    let (lock, lock_hash) = GrammarPackLock::load_with_hash(lock_path)?;
    ensure_full_lock(&lock)?;
    Ok(render_provider_lock(&lock, &lock_hash))
}

/// Render the deterministic full-pack license ledger.
///
/// # Errors
///
/// Returns [`XtaskError`] when the lock cannot be read, hashed, or validated,
/// or when its audited full-pack cardinalities have drifted.
pub fn render_notices(lock_path: impl AsRef<Path>) -> Result<String, XtaskError> {
    let (lock, lock_hash) = GrammarPackLock::load_with_hash(lock_path)?;
    ensure_full_lock(&lock)?;
    Ok(render_notices_lock(&lock, &lock_hash))
}

/// Generate or check the checked-in full-provider registry.
///
/// # Errors
///
/// Returns [`XtaskError`] for invalid locks, output I/O failures, or drift in
/// check mode.
pub fn generate_provider(
    lock_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
    check: bool,
) -> Result<GenerationOutcome, XtaskError> {
    let content = render_provider(lock_path)?;
    write_generated(output_path.as_ref(), &content, check)
}

/// Generate or check the checked-in full-pack license ledger.
///
/// # Errors
///
/// Returns [`XtaskError`] for invalid locks, output I/O failures, or drift in
/// check mode.
pub fn generate_notices(
    lock_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
    check: bool,
) -> Result<GenerationOutcome, XtaskError> {
    let content = render_notices(lock_path)?;
    write_generated(output_path.as_ref(), &content, check)
}

fn ensure_full_lock(lock: &GrammarPackLock) -> Result<(), XtaskError> {
    let mut unavailable = lock.unavailable_language_ids();
    unavailable.sort_unstable();
    let mut orphans = lock.orphan_grammar_names();
    orphans.sort_unstable();
    if lock.grammars.len() != 159
        || lock.language_mappings.len() != 160
        || lock.available_language_count() != 159
        || lock.unique_bound_grammar_count() != 157
        || unavailable != ["nim"]
        || orphans != ["objectscript_routine", "objectscript_udl"]
    {
        return invalid(format!(
            "full grammar inventory drift: grammars={}, ids={}, available={}, unique={}, unavailable={unavailable:?}, orphans={orphans:?}",
            lock.grammars.len(),
            lock.language_mappings.len(),
            lock.available_language_count(),
            lock.unique_bound_grammar_count(),
        ));
    }
    Ok(())
}

fn render_provider_lock(lock: &GrammarPackLock, lock_hash: &str) -> String {
    let grammars_by_name = lock
        .grammars
        .iter()
        .map(|grammar| (grammar.name.as_str(), grammar))
        .collect::<BTreeMap<_, _>>();
    let bound_names = lock
        .language_mappings
        .iter()
        .filter(|mapping| mapping.status == LanguageBindingStatus::Available)
        .map(|mapping| {
            mapping
                .grammar
                .as_deref()
                .expect("validated available mapping")
        })
        .collect::<BTreeSet<_>>();
    let grammar_indices = bound_names
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, index))
        .collect::<BTreeMap<_, _>>();
    let mut mappings = lock.language_mappings.iter().collect::<Vec<_>>();
    mappings.sort_unstable_by(|left, right| {
        left.language_id
            .as_bytes()
            .cmp(right.language_id.as_bytes())
    });

    let mut source = provider_prelude(lock, lock_hash);
    append_factory_registry(&mut source, &bound_names, &grammars_by_name);
    append_grammar_registry(&mut source, &bound_names, &grammars_by_name);
    append_language_registry(&mut source, &mappings, &grammar_indices);
    source.push_str("pub(crate) fn language_fn(grammar_index: usize) -> Option<LanguageFn> {\n");
    source.push_str("    FACTORIES.get(grammar_index).copied().map(|factory| {\n");
    source.push_str(
        "        // SAFETY: every factory is a verified, linked Tree-sitter language entry.\n",
    );
    source.push_str("        unsafe { LanguageFn::from_raw(factory) }\n");
    source.push_str("    })\n");
    source.push_str("}\n");
    source
}

fn provider_prelude(lock: &GrammarPackLock, lock_hash: &str) -> String {
    let mut source = String::new();
    writeln!(source, "// goldeneye-full-pack-lock-sha256: {lock_hash}")
        .expect("writing to a String cannot fail");
    source.push_str("// Generated by cargo xtask grammars generate-provider; do not edit.\n\n");
    source.push_str("use tree_sitter_language::LanguageFn;\n\n");
    writeln!(
        source,
        "pub(crate) const FULL_PACK_LOCK_SHA256: &str = {};",
        rust_string(lock_hash)
    )
    .expect("writing to a String cannot fail");
    writeln!(
        source,
        "pub(crate) const FULL_PACK_UPSTREAM_COMMIT: &str = {};",
        rust_string(lock.upstream_commit())
    )
    .expect("writing to a String cannot fail");
    source.push_str("pub(crate) const DECLARED_LANGUAGE_COUNT: usize = 160;\n");
    source.push_str("pub(crate) const AVAILABLE_LANGUAGE_COUNT: usize = 159;\n");
    source.push_str("pub(crate) const UNIQUE_GRAMMAR_COUNT: usize = 157;\n");
    source.push_str("pub(crate) const COMPILED_SOURCE_COUNT: usize = 159;\n");
    source.push_str("pub(crate) const ORPHAN_SOURCE_COUNT: usize = 2;\n\n");
    source.push_str("#[derive(Clone, Copy)]\n");
    source.push_str("pub(crate) struct GeneratedGrammar {\n");
    source.push_str("    pub(crate) name: &'static str,\n");
    source.push_str("    pub(crate) exported_symbol: &'static str,\n");
    source.push_str("    pub(crate) abi: u32,\n");
    source.push_str("    pub(crate) scanner_language: &'static str,\n");
    source.push_str("    pub(crate) source_hash: &'static str,\n");
    source.push_str("}\n\n");
    source.push_str("#[derive(Clone, Copy)]\n");
    source.push_str("pub(crate) enum GeneratedAvailability {\n");
    source.push_str("    Available { grammar_index: usize },\n");
    source.push_str("    Unavailable { reason: &'static str },\n");
    source.push_str("}\n\n");
    source.push_str("#[derive(Clone, Copy)]\n");
    source.push_str("pub(crate) struct GeneratedLanguage {\n");
    source.push_str("    pub(crate) id: &'static str,\n");
    source.push_str("    pub(crate) availability: GeneratedAvailability,\n");
    source.push_str("}\n\n");
    source
}

fn append_factory_registry(
    source: &mut String,
    bound_names: &BTreeSet<&str>,
    grammars_by_name: &BTreeMap<&str, &GrammarRecord>,
) {
    source.push_str("unsafe extern \"C\" {\n");
    for (ordinal, name) in bound_names.iter().enumerate() {
        let grammar = grammars_by_name
            .get(name)
            .expect("validated mapping references a grammar");
        let link_name = format!("goldeneye_full_{}", grammar.exported_symbol);
        writeln!(source, "    #[link_name = {}]", rust_string(&link_name))
            .expect("writing to a String cannot fail");
        writeln!(source, "    fn grammar_{ordinal:03}() -> *const ();")
            .expect("writing to a String cannot fail");
    }
    source.push_str("}\n\n");
    source.push_str("const FACTORIES: [unsafe extern \"C\" fn() -> *const (); 157] = [\n");
    for ordinal in 0..bound_names.len() {
        writeln!(source, "    grammar_{ordinal:03},").expect("writing to a String cannot fail");
    }
    source.push_str("];\n\n");
}

fn append_grammar_registry(
    source: &mut String,
    bound_names: &BTreeSet<&str>,
    grammars_by_name: &BTreeMap<&str, &GrammarRecord>,
) {
    source.push_str("pub(crate) static GRAMMARS: [GeneratedGrammar; 157] = [\n");
    for name in bound_names {
        let grammar = grammars_by_name
            .get(name)
            .expect("validated mapping references a grammar");
        writeln!(
            source,
            "    GeneratedGrammar {{ name: {}, exported_symbol: {}, abi: {}, scanner_language: {}, source_hash: {} }},",
            rust_string(&grammar.name),
            rust_string(&grammar.exported_symbol),
            grammar.abi,
            rust_string(&grammar.scanner_language),
            rust_string(&grammar.source_hash),
        )
        .expect("writing to a String cannot fail");
    }
    source.push_str("];\n\n");
}

fn append_language_registry(
    source: &mut String,
    mappings: &[&LanguageMapping],
    grammar_indices: &BTreeMap<&str, usize>,
) {
    source.push_str("pub(crate) static LANGUAGES: [GeneratedLanguage; 160] = [\n");
    for mapping in mappings {
        match mapping.status {
            LanguageBindingStatus::Available => {
                let grammar = mapping
                    .grammar
                    .as_deref()
                    .expect("validated available mapping");
                let index = grammar_indices[grammar];
                writeln!(
                    source,
                    "    GeneratedLanguage {{ id: {}, availability: GeneratedAvailability::Available {{ grammar_index: {index} }} }},",
                    rust_string(&mapping.language_id),
                )
                .expect("writing to a String cannot fail");
            }
            LanguageBindingStatus::Unavailable => {
                let reason = mapping
                    .reason
                    .as_deref()
                    .expect("validated unavailable mapping");
                writeln!(
                    source,
                    "    GeneratedLanguage {{ id: {}, availability: GeneratedAvailability::Unavailable {{ reason: {} }} }},",
                    rust_string(&mapping.language_id),
                    rust_string(reason),
                )
                .expect("writing to a String cannot fail");
            }
        }
    }
    source.push_str("];\n\n");
}

fn render_notices_lock(lock: &GrammarPackLock, lock_hash: &str) -> String {
    let mut grammars = lock.grammars.iter().collect::<Vec<_>>();
    grammars.sort_unstable_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));
    let mut native_support_licenses = lock
        .native_support
        .iter()
        .flat_map(|support| {
            support
                .license_files
                .iter()
                .map(move |license| (support, license))
        })
        .collect::<Vec<_>>();
    native_support_licenses.sort_unstable_by(
        |(left_support, left_license), (right_support, right_license)| {
            left_support
                .name
                .as_bytes()
                .cmp(right_support.name.as_bytes())
                .then_with(|| left_license.as_bytes().cmp(right_license.as_bytes()))
        },
    );

    let mut ledger = String::new();
    ledger.push_str("# Full Grammar Pack License Ledger\n\n");
    ledger.push_str("Generated by cargo xtask grammars generate-notices; do not edit.\n\n");
    writeln!(
        ledger,
        "<!-- goldeneye-full-pack-lock-sha256: {lock_hash} -->"
    )
    .expect("writing to a String cannot fail");
    ledger.push('\n');
    ledger.push_str(
        "| Grammar | Repository | Revision or missing-revision reason | Direct license path | Source SHA-256 |\n",
    );
    ledger.push_str("| --- | --- | --- | --- | --- |\n");
    for grammar in grammars {
        let revision = grammar
            .commit
            .as_deref()
            .or(grammar.missing_commit_reason.as_deref())
            .expect("validated grammar provenance");
        writeln!(
            ledger,
            "| {} | {} | {} | {} | {} |",
            html_cell(&grammar.name),
            html_cell(&grammar.repository),
            html_cell(revision),
            html_cell(&format!("{}/LICENSE", grammar.name)),
            html_cell(&grammar.source_hash),
        )
        .expect("writing to a String cannot fail");
    }
    if !native_support_licenses.is_empty() {
        ledger.push_str("\n## Native Support Assets\n\n");
        ledger.push_str(
            "| Support group | Repository | Revision or missing-revision reason | License path | Source SHA-256 |\n",
        );
        ledger.push_str("| --- | --- | --- | --- | --- |\n");
        for (support, license) in native_support_licenses {
            let revision = support
                .commit
                .as_deref()
                .or(support.missing_commit_reason.as_deref())
                .expect("validated native-support provenance");
            writeln!(
                ledger,
                "| {} | {} | {} | {} | {} |",
                html_cell(&support.name),
                html_cell(&support.repository),
                html_cell(revision),
                html_cell(&format!("{}/{}", support.name, license)),
                html_cell(&support.source_hash),
            )
            .expect("writing to a String cannot fail");
        }
    }
    ledger
}

fn rust_string(value: &str) -> String {
    let mut literal = String::from("\"");
    for character in value.chars() {
        match character {
            '\"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            control if control.is_control() => {
                write!(literal, "\\u{{{:x}}}", u32::from(control))
                    .expect("writing to a String cannot fail");
            }
            other => literal.push(other),
        }
    }
    literal.push('\"');
    literal
}

fn html_cell(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;");
    format!("<code>{escaped}</code>")
}

fn write_generated(
    output_path: &Path,
    content: &str,
    check: bool,
) -> Result<GenerationOutcome, XtaskError> {
    let existing = match fs::read(output_path) {
        Ok(existing) => Some(existing),
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(XtaskError::Io {
                path: output_path.to_path_buf(),
                source,
            });
        }
    };
    if existing.as_deref() == Some(content.as_bytes()) {
        return Ok(GenerationOutcome::Unchanged);
    }
    if check {
        return invalid(format!(
            "generated output drift at {}; rerun the matching cargo xtask grammars generator",
            output_path.display()
        ));
    }

    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| XtaskError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary = Builder::new()
        .prefix(".goldeneye-generated-")
        .tempfile_in(parent)
        .map_err(|source| XtaskError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|source| XtaskError::Io {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| XtaskError::Io {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .persist(output_path)
        .map_err(|error| XtaskError::Io {
            path: output_path.to_path_buf(),
            source: error.error,
        })?;
    Ok(GenerationOutcome::Written)
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
    let expected_state = GrammarPackState::expected(lock_path, &lock)?;
    let lock_hash = expected_state.lock_hash().to_owned();
    let destination = prepare_destination(destination_root)?;
    reject_overlap(source.safety_root(), &destination.path)?;

    if destination.exists {
        // A no-op remains source-driven: both the requested source and the
        // already-materialized destination are independently rehashed.
        source.verify(&lock)?;
        verify_existing_pack(lock_path, &lock, &destination.path)?;
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
        verify_materialized_pack(lock_path, &lock, temporary.path())?;
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
    lock_path: &Path,
    lock: &GrammarPackLock,
    destination: &Path,
) -> Result<(), XtaskError> {
    verify_materialized_pack(lock_path, lock, destination)
        .map(|_| ())
        .map_err(|source| XtaskError::ExistingPack { source })
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
