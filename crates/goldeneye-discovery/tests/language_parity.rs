use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use goldeneye_discovery::{LanguageId, LanguageRegistry};

const UPSTREAM_COMMIT: &str = "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c";
const UPSTREAM_REPOSITORY: &str = "https://github.com/DeusData/codebase-memory-mcp";
const CHECKED_IN_REGISTRY: &[u8] = include_bytes!("../data/languages.tsv");

#[test]
fn registry_matches_audited_upstream_counts() {
    let registry = LanguageRegistry::upstream();

    assert_eq!(registry.language_count(), 160);
    assert_eq!(registry.extension_count(), 239);
    assert_eq!(registry.filename_count(), 33);
    assert_eq!(registry.compound_extension_count(), 1);
}

#[test]
fn filename_extension_and_compound_precedence_match_upstream() {
    let registry = LanguageRegistry::upstream();

    assert_eq!(
        registry
            .classify(Path::new("main.rs"))
            .expect(".rs is registered")
            .as_str(),
        "rust"
    );
    assert_eq!(
        registry
            .classify(Path::new("CMakeLists.txt"))
            .expect("CMakeLists.txt is registered")
            .as_str(),
        "cmake"
    );
    assert_eq!(
        registry
            .classify(Path::new(".env"))
            .expect(".env is registered")
            .as_str(),
        "dotenv"
    );
    assert_eq!(
        registry
            .classify(Path::new("view.blade.php"))
            .expect(".blade.php is registered")
            .as_str(),
        "blade"
    );
    assert_eq!(registry.classify(Path::new("unknown.binary")), None);
}

#[test]
fn registry_matching_is_ascii_case_sensitive() {
    let registry = LanguageRegistry::upstream();

    assert_eq!(registry.classify(Path::new("main.RS")), None);
    assert_eq!(registry.classify(Path::new("cmakelists.txt")), None);
}

#[test]
fn explicit_extension_override_wins() {
    let mut overrides = HashMap::new();
    overrides.insert(
        OsString::from(".mjs"),
        LanguageId::new("typescript").expect("valid language ID"),
    );
    let registry = LanguageRegistry::with_overrides(overrides).expect("valid registry data");

    assert_eq!(
        registry
            .classify(Path::new("index.mjs"))
            .expect("override is registered")
            .as_str(),
        "typescript"
    );
}

#[test]
fn explicit_extension_override_precedes_exact_filename() {
    let mut overrides = HashMap::new();
    overrides.insert(
        OsString::from(".txt"),
        LanguageId::new("typescript").expect("valid language ID"),
    );
    let registry = LanguageRegistry::with_overrides(overrides).expect("valid registry data");

    assert_eq!(
        registry
            .classify(Path::new("CMakeLists.txt"))
            .expect("override is registered")
            .as_str(),
        "typescript"
    );
}

#[test]
fn checked_in_registry_records_upstream_provenance() {
    let registry = std::str::from_utf8(CHECKED_IN_REGISTRY).expect("registry is UTF-8");

    assert!(registry.contains(UPSTREAM_REPOSITORY));
    assert!(registry.contains(UPSTREAM_COMMIT));
    assert!(
        !registry.contains("\r\n"),
        "registry must use stable LF endings"
    );
}

#[test]
fn generated_language_data_has_no_trailing_whitespace() {
    for (index, line) in include_str!("../data/languages.tsv").lines().enumerate() {
        assert_eq!(
            line.trim_end(),
            line,
            "trailing whitespace on TSV line {}",
            index + 1
        );
    }
}

#[test]
fn checked_in_registry_has_a_stable_git_lf_policy() {
    let attributes = fs::read_to_string(repository_root().join(".gitattributes"))
        .expect("repository must define line-ending attributes");

    assert!(
        attributes
            .lines()
            .any(|line| { line == "crates/goldeneye-discovery/data/languages.tsv text eol=lf" })
    );
}

#[test]
fn checked_in_registry_is_reproducible_when_upstream_is_available() {
    let repository = repository_root();
    let upstream = repository.join(".upstream/codebase-memory-mcp");
    if !upstream.is_dir() {
        return;
    }

    let output_directory = tempfile::tempdir().expect("temporary output directory");
    let output = output_directory.path().join("languages.tsv");
    let script = repository.join("tools/export_upstream_languages.py");
    let status = run_exporter(&script, &upstream, &output);

    assert!(status.success(), "exporter failed with status {status}");
    assert_eq!(
        fs::read(output).expect("generated registry"),
        CHECKED_IN_REGISTRY
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives below repository root")
        .to_path_buf()
}

fn run_exporter(script: &Path, upstream: &Path, output: &Path) -> std::process::ExitStatus {
    for interpreter in ["python", "python3"] {
        match Command::new(interpreter)
            .arg(script)
            .arg("--upstream")
            .arg(upstream)
            .arg("--output")
            .arg(output)
            .status()
        {
            Ok(status) => return status,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to run {interpreter}: {error}"),
        }
    }
    panic!("python or python3 is required when the upstream checkout is present")
}
