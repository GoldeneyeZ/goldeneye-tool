use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use goldeneye_grammar_pack::{
    GrammarPackLock, GrammarPackState, PACK_STATE_FILE, VerifiedPack, verify_materialized_pack,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const ASSET_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-assets-v1\0";
const UPSTREAM_COMMIT: &str = "1111111111111111111111111111111111111111";

struct Fixture {
    temporary: TempDir,
    lock_path: PathBuf,
    root: PathBuf,
    lock: GrammarPackLock,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let lock_path = temporary.path().join("full-pack.toml");
        let root = temporary.path().join("materialized");
        let grammar_root = root.join("alpha");
        fs::create_dir_all(&grammar_root).unwrap();

        let assets = [
            ("LICENSE", b"fixture license\n".as_slice()),
            ("parser.c", b"parser fixture\n".as_slice()),
        ];
        for (path, bytes) in assets {
            fs::write(grammar_root.join(path), bytes).unwrap();
        }
        let source_hash = independent_hash(&assets);
        fs::write(&lock_path, tiny_lock(&source_hash)).unwrap();
        let lock = GrammarPackLock::load(&lock_path).unwrap();
        write_state(
            &root,
            &GrammarPackState::expected(&lock_path, &lock).unwrap(),
        );

        Self {
            temporary,
            lock_path,
            root,
            lock,
        }
    }

    fn verify(&self) -> Result<VerifiedPack, goldeneye_grammar_pack::PackError> {
        verify_materialized_pack(&self.lock_path, &self.lock, &self.root)
    }
}

#[test]
fn exact_state_layout_and_hash_match_is_verified() {
    let fixture = Fixture::new();

    assert_eq!(
        fixture.verify().unwrap(),
        VerifiedPack {
            grammar_count: 1,
            asset_count: 2,
        }
    );
}

#[test]
fn mismatched_lock_hash_is_rejected() {
    let fixture = Fixture::new();
    let mut state = state_value(&fixture);
    state["lock_hash"] = Value::String("0".repeat(64));
    write_json(&fixture.root.join(PACK_STATE_FILE), &state);

    assert!(fixture.verify().is_err());
}

#[test]
fn invalid_state_json_is_rejected() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join(PACK_STATE_FILE), b"{not json\n").unwrap();

    assert!(fixture.verify().is_err());
}

#[test]
fn unknown_state_fields_are_rejected() {
    let fixture = Fixture::new();
    let mut state = state_value(&fixture);
    state
        .as_object_mut()
        .unwrap()
        .insert("unexpected".into(), Value::Bool(true));
    write_json(&fixture.root.join(PACK_STATE_FILE), &state);

    assert!(fixture.verify().is_err());
}

#[test]
fn missing_asset_is_rejected() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join("alpha/parser.c")).unwrap();

    assert!(fixture.verify().is_err());
}

#[test]
fn extra_file_is_rejected() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("alpha/extra.c"), b"unexpected\n").unwrap();

    assert!(fixture.verify().is_err());
}

#[test]
fn extra_directory_is_rejected() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("unexpected")).unwrap();

    assert!(fixture.verify().is_err());
}

#[test]
fn final_asset_symlink_is_rejected() {
    let fixture = Fixture::new();
    let parser = fixture.root.join("alpha/parser.c");
    let outside = fixture.temporary.path().join("outside-parser.c");
    fs::write(&outside, b"external parser\n").unwrap();
    fs::remove_file(&parser).unwrap();
    create_final_link_or_reparse(&outside, &parser)
        .expect("failed to create required final link/reparse fixture");
    assert!(is_link_or_reparse(&parser));

    assert!(fixture.verify().is_err());
}

#[test]
fn intermediate_directory_symlink_is_rejected() {
    let fixture = Fixture::new();
    let grammar_root = fixture.root.join("alpha");
    let outside = fixture.temporary.path().join("outside-alpha");
    fs::rename(&grammar_root, &outside).unwrap();
    create_directory_link_or_reparse(&outside, &grammar_root)
        .expect("failed to create required directory link/reparse fixture");
    assert!(is_link_or_reparse(&grammar_root));

    assert!(fixture.verify().is_err());
}

#[test]
fn same_size_modified_asset_is_rejected() {
    let fixture = Fixture::new();
    let parser = fixture.root.join("alpha/parser.c");
    let mut modified = fs::read(&parser).unwrap();
    modified[0] ^= 1;
    fs::write(parser, modified).unwrap();

    assert!(fixture.verify().is_err());
}

#[test]
fn repeated_verification_does_not_mutate_a_valid_cache() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.root);

    let first = fixture.verify().unwrap();
    let second = fixture.verify().unwrap();

    assert_eq!(first, second);
    assert_eq!(snapshot(&fixture.root), before);
}

fn state_value(fixture: &Fixture) -> Value {
    serde_json::to_value(GrammarPackState::expected(&fixture.lock_path, &fixture.lock).unwrap())
        .unwrap()
}

fn write_state(root: &Path, state: &GrammarPackState) {
    write_json(&root.join(PACK_STATE_FILE), state);
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

fn independent_hash(assets: &[(&str, &[u8])]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ASSET_HASH_DOMAIN);
    for (path, bytes) in assets {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

fn tiny_lock(source_hash: &str) -> String {
    format!(
        r#"schema_version = 1
upstream_repository = "https://example.invalid/upstream"
upstream_commit = "{UPSTREAM_COMMIT}"
declared_grammar_count = 1
declared_language_binding_count = 1
compatible_abi_min = 13
compatible_abi_max = 15
hash_algorithm = "sha256"
hash_domain = "goldeneye-grammar-assets-v1"

[[grammars]]
name = "alpha"
repository = "https://example.invalid/alpha"
commit = "2222222222222222222222222222222222222222"
abi = 15
assets = ["LICENSE", "parser.c"]
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

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries = fs::read_dir(directory)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[cfg(unix)]
fn create_final_link_or_reparse(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_final_link_or_reparse(target: &Path, link: &Path) -> std::io::Result<()> {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            fs::remove_file(target)?;
            let junction_target = target.with_file_name("final-reparse-target");
            let staging_link = link.with_file_name("final-reparse-link");
            fs::create_dir(&junction_target)?;
            create_directory_junction(&junction_target, &staging_link)?;
            fs::rename(staging_link, link)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn create_directory_link_or_reparse(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link_or_reparse(target: &Path, link: &Path) -> std::io::Result<()> {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            create_directory_junction(target, link)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn is_link_or_reparse(path: &Path) -> bool {
    fs::symlink_metadata(path).unwrap().file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_or_reparse(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let metadata = fs::symlink_metadata(path).unwrap();
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
fn create_directory_junction(target: &Path, link: &Path) -> std::io::Result<()> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    Err(std::io::Error::other(format!(
        "mklink /J failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )))
}
