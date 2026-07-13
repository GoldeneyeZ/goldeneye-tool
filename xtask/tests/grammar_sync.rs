use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use goldeneye_grammar_pack::{GrammarRecord, hash_grammar_assets, lock_file_hash};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use xtask::{SyncOutcome, sync_git_grammars, sync_grammars, verify_git_grammars, verify_grammars};

const TEST_COMMIT: &str = "1111111111111111111111111111111111111111";
const ASSET_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-assets-v1\0";

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    source: PathBuf,
    lock: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().to_path_buf();
        let source = root.join("source");
        write(&source.join("alpha/LICENSE"), b"MIT\n");
        write(&source.join("alpha/parser.c"), b"alpha parser\n");
        write(&source.join("beta/LICENSE"), b"Apache-2.0\n");
        write(&source.join("beta/parser.c"), b"beta parser\n");
        write(&source.join("beta/scanner.c"), b"beta scanner\n");

        let alpha_hash = independent_hash(&source.join("alpha"), &["LICENSE", "parser.c"]);
        let beta_hash =
            independent_hash(&source.join("beta"), &["LICENSE", "parser.c", "scanner.c"]);
        let lock = root.join("full-pack.toml");
        write(&lock, tiny_lock(&alpha_hash, &beta_hash).as_bytes());

        Self {
            _temporary: temporary,
            root,
            source,
            lock,
        }
    }

    fn destination(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

#[test]
fn hash_framing_matches_pinned_golden() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("one/LICENSE"), b"MIT\n");
    let grammar = GrammarRecord {
        name: "one".into(),
        repository: "https://example.invalid/one".into(),
        commit: Some(TEST_COMMIT.into()),
        missing_commit_reason: None,
        abi: 15,
        assets: vec!["LICENSE".into()],
        source_hash: "a78de479071a0eb45f0990913847f457d8c37cfea62ebcdc16cb6fbdaaae8868".into(),
        scanner_language: "none".into(),
        license_files: vec!["LICENSE".into()],
        verdict: "fixture".into(),
        provenance_notes: Vec::new(),
        orphan_reason: None,
    };

    assert_eq!(
        hash_grammar_assets(temporary.path(), &grammar).unwrap(),
        grammar.source_hash
    );
}

#[test]
fn verify_and_sync_clean_pack_without_mutating_source() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    let source_before = snapshot(&fixture.source);

    let verified = verify_grammars(&fixture.lock, &fixture.source).unwrap();
    assert_eq!(verified.grammar_count, 2);
    assert_eq!(verified.asset_count, 5);
    assert_eq!(
        sync_grammars(&fixture.lock, &fixture.source, &destination).unwrap(),
        SyncOutcome::Created
    );

    assert_eq!(snapshot(&fixture.source), source_before);
    assert_eq!(
        fs::read(destination.join("alpha/parser.c")).unwrap(),
        b"alpha parser\n"
    );
    assert!(destination.join("pack-state.json").is_file());
}

#[test]
fn git_source_uses_lf_blobs_while_directory_source_sees_crlf_smudge() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Fixture"]);
    git(&repository, &["config", "core.autocrlf", "true"]);

    let prefix = "vendor/grammars";
    let source = repository.join(prefix);
    write(&source.join("alpha/LICENSE"), b"MIT\r\n");
    write(&source.join("alpha/parser.c"), b"alpha parser\r\n");
    git(&repository, &["add", "vendor/grammars"]);
    git(
        &repository,
        &[
            "update-index",
            "--chmod=+x",
            "vendor/grammars/alpha/parser.c",
        ],
    );
    git(&repository, &["commit", "--quiet", "-m", "fixture"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    let source_hash =
        independent_hash_bytes(&[("LICENSE", b"MIT\n"), ("parser.c", b"alpha parser\n")]);
    let lock = temporary.path().join("git-pack.toml");
    write(
        &lock,
        one_grammar_lock(&commit, &source_hash, &["LICENSE", "parser.c"]).as_bytes(),
    );
    let destination = temporary.path().join("pack");

    let directory_error = verify_grammars(&lock, &source).unwrap_err().to_string();
    assert!(
        directory_error.contains("hash mismatch"),
        "{directory_error}"
    );

    let verified = verify_git_grammars(&lock, &repository, prefix).unwrap();
    assert_eq!(verified.grammar_count, 1);
    assert_eq!(verified.asset_count, 2);
    assert_eq!(
        sync_git_grammars(&lock, &repository, prefix, &destination).unwrap(),
        SyncOutcome::Created
    );
    assert_eq!(
        fs::read(destination.join("alpha/parser.c")).unwrap(),
        b"alpha parser\n"
    );
    assert_eq!(
        fs::read(source.join("alpha/parser.c")).unwrap(),
        b"alpha parser\r\n"
    );
}

#[test]
fn git_source_ignores_replacement_refs_for_locked_commit() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Fixture"]);
    let prefix = "vendor/grammars";
    let source = repository.join(prefix);
    write(&source.join("alpha/LICENSE"), b"MIT\n");
    write(&source.join("alpha/parser.c"), b"original parser\n");
    git(&repository, &["add", "vendor/grammars"]);
    git(&repository, &["commit", "--quiet", "-m", "original"]);
    let original = git(&repository, &["rev-parse", "HEAD"]);
    let source_hash =
        independent_hash_bytes(&[("LICENSE", b"MIT\n"), ("parser.c", b"original parser\n")]);

    write(&source.join("alpha/parser.c"), b"replacement parser\n");
    git(&repository, &["add", "vendor/grammars/alpha/parser.c"]);
    git(&repository, &["commit", "--quiet", "-m", "replacement"]);
    let replacement = git(&repository, &["rev-parse", "HEAD"]);
    git(&repository, &["replace", &original, &replacement]);

    let lock = temporary.path().join("git-pack.toml");
    write(
        &lock,
        one_grammar_lock(&original, &source_hash, &["LICENSE", "parser.c"]).as_bytes(),
    );
    let destination = temporary.path().join("pack");

    verify_git_grammars(&lock, &repository, prefix).unwrap();
    sync_git_grammars(&lock, &repository, prefix, &destination).unwrap();
    assert_eq!(
        fs::read(destination.join("alpha/parser.c")).unwrap(),
        b"original parser\n"
    );
}

#[test]
fn git_source_rejects_non_regular_modes() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Fixture"]);
    write(&repository.join("vendor/grammars/alpha/LICENSE"), b"MIT\n");
    write(&repository.join("link-target"), b"outside.c");
    git(&repository, &["add", "vendor/grammars/alpha/LICENSE"]);
    let object_id = git(&repository, &["hash-object", "-w", "link-target"]);
    git(
        &repository,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{object_id},vendor/grammars/alpha/parser.c"),
        ],
    );
    git(&repository, &["commit", "--quiet", "-m", "fixture"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    let source_hash = independent_hash_bytes(&[("LICENSE", b"MIT\n"), ("parser.c", b"outside.c")]);
    let lock = temporary.path().join("git-pack.toml");
    write(
        &lock,
        one_grammar_lock(&commit, &source_hash, &["LICENSE", "parser.c"]).as_bytes(),
    );

    let error = verify_git_grammars(&lock, &repository, "vendor/grammars")
        .unwrap_err()
        .to_string();
    assert!(error.contains("regular Git blob"), "{error}");
}

#[test]
fn git_source_streams_assets_larger_than_two_copy_buffers() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet"]);
    git(
        &repository,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Fixture"]);
    git(&repository, &["config", "core.autocrlf", "false"]);
    let large_parser = vec![b'x'; 2 * 1024 * 1024 + 17];
    write(&repository.join("vendor/grammars/alpha/LICENSE"), b"MIT\n");
    write(
        &repository.join("vendor/grammars/alpha/parser.c"),
        &large_parser,
    );
    git(&repository, &["add", "vendor/grammars"]);
    git(&repository, &["commit", "--quiet", "-m", "fixture"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    let source_hash = independent_hash_bytes(&[("LICENSE", b"MIT\n"), ("parser.c", &large_parser)]);
    let lock = temporary.path().join("git-pack.toml");
    write(
        &lock,
        one_grammar_lock(&commit, &source_hash, &["LICENSE", "parser.c"]).as_bytes(),
    );
    let destination = temporary.path().join("pack");

    verify_git_grammars(&lock, &repository, "vendor/grammars").unwrap();
    sync_git_grammars(&lock, &repository, "vendor/grammars", &destination).unwrap();
    assert_eq!(
        fs::read(destination.join("alpha/parser.c")).unwrap(),
        large_parser
    );
}

#[test]
fn verify_rejects_hash_mismatch_and_sync_publishes_nothing() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    write(&fixture.source.join("alpha/parser.c"), b"tampered\n");

    let error = verify_grammars(&fixture.lock, &fixture.source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("hash mismatch"), "{error}");
    let error = sync_grammars(&fixture.lock, &fixture.source, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("hash mismatch"), "{error}");
    assert!(!destination.exists());
}

#[test]
fn verify_rejects_missing_license() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.source.join("alpha/LICENSE")).unwrap();

    let error = verify_grammars(&fixture.lock, &fixture.source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("LICENSE"), "{error}");
}

#[test]
fn lock_rejects_traversal_before_reading_source() {
    let fixture = Fixture::new();
    let source = fs::read_to_string(&fixture.lock).unwrap();
    let invalid = source.replace(
        "assets = [\"LICENSE\", \"parser.c\"]",
        "assets = [\"../escape\", \"LICENSE\", \"parser.c\"]",
    );
    write(&fixture.lock, invalid.as_bytes());

    let error = verify_grammars(&fixture.lock, &fixture.source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsafe path component"), "{error}");
}

#[test]
fn sync_cleans_only_marker_owned_stale_temporary_directories() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    let lock_hash = lock_file_hash(&fixture.lock).unwrap();
    let owned = fixture.root.join(".pack.goldeneye-tmp-owned");
    let unowned = fixture.root.join(".pack.goldeneye-tmp-unowned");
    fs::create_dir(&owned).unwrap();
    fs::create_dir(&unowned).unwrap();
    write(
        &owned.join(".goldeneye-owned-temp.json"),
        (json!({
            "schema_version": 1,
            "destination": "pack",
            "lock_hash": lock_hash,
        })
        .to_string()
            + "\n")
            .as_bytes(),
    );
    write(&unowned.join("sentinel"), b"leave me");

    assert_eq!(
        sync_grammars(&fixture.lock, &fixture.source, &destination).unwrap(),
        SyncOutcome::Created
    );
    assert!(!owned.exists());
    assert_eq!(fs::read(unowned.join("sentinel")).unwrap(), b"leave me");
}

#[test]
fn identical_existing_pack_is_reverified_and_returns_no_op() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    assert_eq!(
        sync_grammars(&fixture.lock, &fixture.source, &destination).unwrap(),
        SyncOutcome::Created
    );
    let before = snapshot(&destination);

    assert_eq!(
        sync_grammars(&fixture.lock, &fixture.source, &destination).unwrap(),
        SyncOutcome::AlreadyCurrent
    );
    assert_eq!(snapshot(&destination), before);
}

#[test]
fn matching_pack_state_never_hides_tampered_materialized_asset() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    sync_grammars(&fixture.lock, &fixture.source, &destination).unwrap();
    write(&destination.join("alpha/parser.c"), b"tampered pack\n");
    let before = snapshot(&destination);

    let error = sync_grammars(&fixture.lock, &fixture.source, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("hash mismatch"), "{error}");
    assert_eq!(snapshot(&destination), before);
}

#[test]
fn existing_non_pack_or_mismatched_pack_is_untouched() {
    let fixture = Fixture::new();
    let destination = fixture.destination("pack");
    fs::create_dir(&destination).unwrap();
    write(&destination.join("sentinel"), b"do not change");
    let before = snapshot(&destination);

    let error = sync_grammars(&fixture.lock, &fixture.source, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("existing destination"), "{error}");
    assert_eq!(snapshot(&destination), before);

    let second = fixture.destination("pack-two");
    sync_grammars(&fixture.lock, &fixture.source, &second).unwrap();
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(second.join("pack-state.json")).unwrap()).unwrap();
    state["lock_hash"] = serde_json::Value::String("0".repeat(64));
    write(
        &second.join("pack-state.json"),
        (serde_json::to_string(&state).unwrap() + "\n").as_bytes(),
    );
    let before = snapshot(&second);
    assert!(sync_grammars(&fixture.lock, &fixture.source, &second).is_err());
    assert_eq!(snapshot(&second), before);
}

#[test]
fn repeated_materialization_is_byte_deterministic() {
    let fixture = Fixture::new();
    let first = fixture.destination("first");
    let second = fixture.destination("second");

    sync_grammars(&fixture.lock, &fixture.source, &first).unwrap();
    sync_grammars(&fixture.lock, &fixture.source, &second).unwrap();

    assert_eq!(snapshot(&first), snapshot(&second));
}

#[test]
fn sync_rejects_source_destination_overlap_in_both_directions() {
    let fixture = Fixture::new();
    let nested_destination = fixture.source.join("nested-pack");
    let error = sync_grammars(&fixture.lock, &fixture.source, &nested_destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("overlap"), "{error}");

    let containing_destination = fixture.root.clone();
    let before = snapshot(&fixture.root);
    let error = sync_grammars(&fixture.lock, &fixture.source, &containing_destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("overlap"), "{error}");
    assert_eq!(snapshot(&fixture.root), before);
}

fn tiny_lock(alpha_hash: &str, beta_hash: &str) -> String {
    format!(
        r#"schema_version = 1
upstream_repository = "https://example.invalid/upstream"
upstream_commit = "{TEST_COMMIT}"
declared_grammar_count = 2
declared_language_binding_count = 2
compatible_abi_min = 13
compatible_abi_max = 15
hash_algorithm = "sha256"
hash_domain = "goldeneye-grammar-assets-v1"

[[grammars]]
name = "alpha"
repository = "https://example.invalid/alpha"
commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
abi = 14
assets = ["LICENSE", "parser.c"]
source_hash = "{alpha_hash}"
scanner_language = "none"
license_files = ["LICENSE"]
verdict = "fixture"
provenance_notes = []

[[grammars]]
name = "beta"
repository = "https://example.invalid/beta"
commit = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
abi = 15
assets = ["LICENSE", "parser.c", "scanner.c"]
source_hash = "{beta_hash}"
scanner_language = "c"
license_files = ["LICENSE"]
verdict = "fixture"
provenance_notes = []

[[language_mappings]]
language_id = "alpha"
status = "available"
grammar = "alpha"

[[language_mappings]]
language_id = "beta"
status = "available"
grammar = "beta"
"#
    )
}

fn one_grammar_lock(commit: &str, source_hash: &str, assets: &[&str]) -> String {
    let assets = assets
        .iter()
        .map(|asset| format!("\"{asset}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"schema_version = 1
upstream_repository = "https://example.invalid/upstream"
upstream_commit = "{commit}"
declared_grammar_count = 1
declared_language_binding_count = 1
compatible_abi_min = 13
compatible_abi_max = 15
hash_algorithm = "sha256"
hash_domain = "goldeneye-grammar-assets-v1"

[[grammars]]
name = "alpha"
repository = "https://example.invalid/alpha"
commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
abi = 14
assets = [{assets}]
source_hash = "{source_hash}"
scanner_language = "none"
license_files = ["LICENSE"]
verdict = "fixture"
provenance_notes = []

[[language_mappings]]
language_id = "alpha"
status = "available"
grammar = "alpha"
"#
    )
}

fn independent_hash(root: &Path, assets: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ASSET_HASH_DOMAIN);
    for asset in assets {
        let content = fs::read(root.join(asset)).unwrap();
        hasher.update((asset.len() as u64).to_be_bytes());
        hasher.update(asset.as_bytes());
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
    }
    format!("{:x}", hasher.finalize())
}

fn independent_hash_bytes(assets: &[(&str, &[u8])]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ASSET_HASH_DOMAIN);
    for (asset, content) in assets {
        hasher.update((asset.len() as u64).to_be_bytes());
        hasher.update(asset.as_bytes());
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
    }
    format!("{:x}", hasher.finalize())
}

fn git(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    file.write_all(content).unwrap();
    file.flush().unwrap();
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut snapshot = BTreeMap::new();
    if !root.exists() {
        return snapshot;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                snapshot.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    snapshot
}
