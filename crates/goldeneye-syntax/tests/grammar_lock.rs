use std::collections::BTreeMap;
use std::path::PathBuf;

use goldeneye_grammar_pack::GrammarPackLock as PackCrateGrammarPackLock;
use goldeneye_syntax::GrammarPackLock;

const MINIMAL_LOCK: &str = r#"
schema_version = 1
upstream_repository = "https://example.invalid/upstream"
upstream_commit = "1111111111111111111111111111111111111111"
declared_grammar_count = 1
declared_language_binding_count = 1
compatible_abi_min = 13
compatible_abi_max = 15
hash_algorithm = "sha256"
hash_domain = "goldeneye-grammar-assets-v1"

[[grammars]]
name = "alpha"
repository = "https://example.invalid/grammar"
commit = "2222222222222222222222222222222222222222"
abi = 15
assets = ["LICENSE", "parser.c"]
source_hash = "0000000000000000000000000000000000000000000000000000000000000000"
scanner_language = "none"
license_files = ["LICENSE"]
verdict = "fixture"
provenance_notes = []

[[language_mappings]]
language_id = "alpha"
status = "available"
grammar = "alpha"
"#;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("syntax crate is inside the workspace")
        .to_path_buf()
}

#[test]
fn syntax_reexport_is_the_pack_crate_type() {
    fn accepts(_: goldeneye_syntax::GrammarPackLock) {}

    let lock =
        PackCrateGrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap();
    accepts(lock);
}

#[test]
fn full_pack_lock_matches_audited_upstream() {
    let lock = GrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap();

    assert_eq!(
        lock.upstream_commit(),
        "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c"
    );
    assert_eq!(lock.grammars.len(), 159);
    assert_eq!(lock.language_mappings.len(), 160);
    assert_eq!(
        lock.abi_histogram(),
        BTreeMap::from([(13, 9), (14, 78), (15, 72)])
    );
    assert_eq!(lock.available_language_count(), 159);
    assert_eq!(lock.unique_bound_grammar_count(), 157);
    assert_eq!(lock.unavailable_language_ids(), ["nim"]);
    assert_eq!(
        lock.orphan_grammar_names(),
        ["objectscript_routine", "objectscript_udl"]
    );
    assert_eq!(lock.grammar_name_for("yaml").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("kustomize").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("k8s").unwrap(), "yaml");
    assert!(
        lock.grammars
            .iter()
            .all(|grammar| !grammar.source_hash.is_empty())
    );
    assert!(
        lock.grammars
            .iter()
            .all(|grammar| !grammar.license_files.is_empty())
    );
}

#[test]
fn lock_rejects_path_separator_in_identifier() {
    let source = MINIMAL_LOCK.replace("\"alpha\"", "\"bad/name\"");

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();
    assert!(error.contains("path component"), "{error}");
}

#[test]
fn lock_rejects_non_compilation_asset() {
    let source = MINIMAL_LOCK.replace(
        "assets = [\"LICENSE\", \"parser.c\"]",
        "assets = [\"LICENSE\", \"README.md\", \"parser.c\"]",
    );

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();
    assert!(error.contains("unsupported asset"), "{error}");
}

#[test]
fn lock_requires_exactly_one_direct_license() {
    let source = MINIMAL_LOCK
        .replace(
            "assets = [\"LICENSE\", \"parser.c\"]",
            "assets = [\"licenses/LICENSE\", \"parser.c\"]",
        )
        .replace(
            "license_files = [\"LICENSE\"]",
            "license_files = [\"licenses/LICENSE\"]",
        );

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();
    assert!(error.contains("direct LICENSE"), "{error}");
}

#[test]
fn lock_requires_direct_parser_asset() {
    let source = MINIMAL_LOCK.replace(
        "assets = [\"LICENSE\", \"parser.c\"]",
        "assets = [\"LICENSE\", \"scanner.c\"]",
    );

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();
    assert!(error.contains("direct parser.c"), "{error}");
}

#[test]
fn source_verification_rejects_final_asset_symlink() {
    use std::fs;

    let temporary = tempfile::tempdir().unwrap();
    let source_root = temporary.path().join("source");
    let grammar_root = source_root.join("alpha");
    let outside = temporary.path().join("outside-parser.c");
    fs::create_dir_all(&grammar_root).unwrap();
    fs::write(grammar_root.join("LICENSE"), b"fixture license").unwrap();
    fs::write(&outside, b"external parser").unwrap();
    if create_file_symlink(&outside, &grammar_root.join("parser.c")).is_err() {
        return;
    }

    let lock = GrammarPackLock::parse(MINIMAL_LOCK).unwrap();
    assert!(lock.verify_source(&source_root).is_err());
}

#[test]
fn source_verification_rejects_intermediate_directory_symlink() {
    use std::fs;

    let temporary = tempfile::tempdir().unwrap();
    let source_root = temporary.path().join("source");
    let outside_grammar = temporary.path().join("outside-alpha");
    fs::create_dir_all(&source_root).unwrap();
    fs::create_dir_all(&outside_grammar).unwrap();
    fs::write(outside_grammar.join("LICENSE"), b"fixture license").unwrap();
    fs::write(outside_grammar.join("parser.c"), b"external parser").unwrap();
    if create_directory_symlink(&outside_grammar, &source_root.join("alpha")).is_err() {
        return;
    }

    let lock = GrammarPackLock::parse(MINIMAL_LOCK).unwrap();
    assert!(lock.verify_source(&source_root).is_err());
}

#[cfg(unix)]
fn create_file_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn create_directory_symlink(
    target: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_symlink(
    target: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}
