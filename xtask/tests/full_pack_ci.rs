use std::fs;
use std::path::{Path, PathBuf};

const UPSTREAM_COMMIT: &str = "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c";
const LOCK_HASH: &str = "ce668d1c07d4f7dd72fd8f167f94d218bfc933a1ccd9ffa52277354968c950c1";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is inside the workspace")
        .to_path_buf()
}

fn tracked_text(path: impl AsRef<Path>) -> String {
    let path = workspace_root().join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read tracked file {}: {error}", path.display()))
}

#[track_caller]
fn require(source: &str, needle: &str) {
    assert!(
        source.contains(needle),
        "missing tracked contract: {needle}"
    );
}

#[track_caller]
fn require_words(source: &str, needle: &str) {
    let normalized = source.split_whitespace().collect::<Vec<_>>().join(" ");
    require(&normalized, needle);
}

#[track_caller]
fn require_before(source: &str, earlier: &str, later: &str) {
    let earlier = source
        .find(earlier)
        .unwrap_or_else(|| panic!("missing earlier contract: {earlier}"));
    let later = source
        .find(later)
        .unwrap_or_else(|| panic!("missing later contract: {later}"));
    assert!(earlier < later, "{earlier} must precede {later}");
}

fn workflow_jobs(workflow: &str) -> (&str, &str) {
    let (_, jobs) = workflow
        .split_once("  rust:\n")
        .expect("workflow retains the default rust job");
    jobs.split_once("  full-pack:\n")
        .expect("workflow adds a separate full-pack job after the default matrix")
}

#[test]
fn default_matrix_remains_core_only_and_cache_free() {
    let workflow = tracked_text(".github/workflows/ci.yml");
    let (default_job, full_job) = workflow_jobs(&workflow);

    require(
        default_job,
        "matrix:\n        os: [ubuntu-latest, windows-latest, macos-latest]",
    );
    for command in [
        "cargo fmt --check",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo test --workspace",
    ] {
        require(default_job, command);
    }
    for forbidden in [
        "GOLDENEYE_GRAMMAR_PACK_DIR",
        "CARGO_NET_OFFLINE",
        "full-grammar-pack",
        "goldeneye-full-grammars",
    ] {
        assert!(
            !default_job.contains(forbidden),
            "default matrix must remain core-only and cache-free: {forbidden}"
        );
    }
    require(full_job, "needs: rust");
}

#[test]
fn full_pack_job_acquires_exact_inputs_then_crosses_one_offline_boundary() {
    let workflow = tracked_text(".github/workflows/ci.yml");
    let (_, full_job) = workflow_jobs(&workflow);

    for contract in [
        "runs-on: ubuntu-latest",
        "repository: DeusData/codebase-memory-mcp",
        &format!("ref: {UPSTREAM_COMMIT}"),
        "path: .upstream/codebase-memory-mcp",
        "dtolnay/rust-toolchain@1.97.0",
        "components: rustfmt, clippy",
        "cargo fetch --locked",
        "CARGO_NET_OFFLINE=true",
        "GOLDENEYE_GRAMMAR_PACK_DIR=target/goldeneye-grammars",
    ] {
        require(full_job, contract);
    }
    require_before(
        full_job,
        "repository: DeusData/codebase-memory-mcp",
        "cargo fetch --locked",
    );
    require_before(full_job, "cargo fetch --locked", "CARGO_NET_OFFLINE=true");

    let cargo_commands = full_job
        .match_indices("cargo ")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let offline_boundary = full_job.find("CARGO_NET_OFFLINE=true").unwrap();
    assert_eq!(cargo_commands.len(), 12);
    assert!(full_job[cargo_commands[0]..].starts_with("cargo fetch --locked"));
    assert!(
        cargo_commands[1..]
            .iter()
            .all(|position| *position > offline_boundary),
        "every full-pack Cargo command after fetch must run offline"
    );
    assert!(!full_job.contains("actions/cache"));
    assert!(!full_job.contains("restore-keys"));
}

#[test]
fn full_pack_job_reproduces_verifies_builds_and_guards_claims() {
    let workflow = tracked_text(".github/workflows/ci.yml");
    let (_, full_job) = workflow_jobs(&workflow);

    for command in [
        "python tools/export_grammar_lock.py --check",
        "--source .upstream/codebase-memory-mcp",
        &format!("--expected-commit {UPSTREAM_COMMIT}"),
        "--output grammars/full-pack.toml",
        "cargo xtask grammars sync",
        "--git-repo .upstream/codebase-memory-mcp",
        "--git-prefix internal/cbm/vendored/grammars",
        "--dest target/goldeneye-grammars",
        "cargo xtask grammars verify",
        "--source target/goldeneye-grammars",
        "cargo xtask grammars generate-provider",
        "--output crates/goldeneye-full-grammars/src/generated.rs --check",
        "cargo xtask grammars generate-notices",
        "--output grammars/full-pack-license-ledger.md --check",
        "cargo clippy -p goldeneye-full-grammars --all-targets --features compiled -- -D warnings",
        "cargo test -p goldeneye-full-grammars --features compiled",
        "cargo clippy -p goldeneye-syntax --all-targets --no-default-features --features full-grammar-pack -- -D warnings",
        "cargo test -p goldeneye-syntax --no-default-features --features full-grammar-pack",
        "cargo test -p goldeneye-syntax --all-features",
        "cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features",
        "cargo test -p goldeneye-syntax --release --no-default-features --features full-grammar-pack --no-run",
    ] {
        require(full_job, command);
    }
    require_before(full_job, "export_grammar_lock.py", "grammars sync");
    require_before(full_job, "grammars sync", "grammars verify");
    require_before(full_job, "grammars verify", "generate-provider");
    require_before(full_job, "generate-provider", "generate-notices");
    require_before(
        full_job,
        "generate-notices",
        "clippy -p goldeneye-full-grammars",
    );
}

#[test]
fn operator_document_has_copyable_recovery_and_bounded_claims() {
    let guide = tracked_text("docs/full-grammar-pack.md");

    for contract in [
        "## PowerShell",
        "## POSIX shell",
        "git clone --filter=blob:none --no-checkout",
        UPSTREAM_COMMIT,
        "cargo fetch --locked",
        "cargo xtask grammars sync",
        "cargo xtask grammars verify",
        "cargo xtask grammars generate-provider",
        "cargo xtask grammars generate-notices",
        "GOLDENEYE_GRAMMAR_PACK_DIR",
        "CARGO_NET_OFFLINE",
        "--no-default-features --features full-grammar-pack",
        "--all-features",
        "160 declared language IDs",
        "159 available language IDs",
        "157 unique callable factories",
        "two ObjectScript orphan sources",
        "159 grammar groups",
        "one native-support group",
        "914 total assets",
        "two native-support license rows",
        "goldeneye_full_",
        "No build-time download",
        "shared `common` native-support assets",
        "MSVC-only COBOL",
        "Phase 6",
        "does not prove broad behavioral conformance",
        "No upstream application C code",
        "no bundled Tree-sitter runtime",
    ] {
        require_words(&guide, contract);
    }
}

#[test]
fn third_party_boundaries_and_license_ledger_match_the_verified_pack() {
    let third_party = tracked_text("THIRD_PARTY.md");
    for contract in [
        "159 grammar groups",
        "one native-support group",
        "914 compilation/license assets",
        "two native-support license rows",
        "shared `common` native-support assets",
        "No upstream application C code",
        "no bundled Tree-sitter runtime",
        "Phase 6",
        "A core-only build is not evidence for the 160-ID full registry",
        "does not prove broad behavioral conformance",
    ] {
        require_words(&third_party, contract);
    }
    assert!(!third_party.contains("907 compilation/license files"));

    let ledger = tracked_text("grammars/full-pack-license-ledger.md");
    require(
        &ledger,
        &format!("<!-- goldeneye-full-pack-lock-sha256: {LOCK_HASH} -->"),
    );
    let (grammar_ledger, support_ledger) = ledger
        .split_once("## Native Support Assets")
        .expect("license ledger has a separate native-support section");
    assert_eq!(
        grammar_ledger
            .lines()
            .filter(|line| line.starts_with("| <code>") && line.contains("</code> |"))
            .count(),
        159
    );
    assert_eq!(
        support_ledger
            .lines()
            .filter(|line| line.starts_with("| <code>common</code> |"))
            .count(),
        2
    );
    require(support_ledger, "<code>common/LICENSE</code>");
    require(support_ledger, "<code>common/tree_sitter/LICENSE</code>");
}
