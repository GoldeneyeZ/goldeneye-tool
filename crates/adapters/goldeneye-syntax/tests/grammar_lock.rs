use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use goldeneye_grammar_pack::{GrammarPackLock as PackCrateGrammarPackLock, lock_file_hash};
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
exported_symbol = "tree_sitter_alpha"
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
fn load_with_hash_hashes_the_exact_bytes_it_parses() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("lock.toml");
    std::fs::write(&path, MINIMAL_LOCK).unwrap();

    let (lock, parsed_hash) = GrammarPackLock::load_with_hash(&path).unwrap();

    assert_eq!(lock.grammars.len(), 1);
    assert_eq!(parsed_hash, lock_file_hash(&path).unwrap());
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
    assert_eq!(
        BTreeMap::from([
            ("csharp", "c_sharp"),
            ("dlang", "d"),
            ("emacslisp", "elisp"),
            ("k8s", "yaml"),
            ("kustomize", "yaml"),
            ("llvm_ir", "llvm"),
            ("makefile", "make"),
            ("vimscript", "vim"),
        ]),
        [
            "csharp",
            "dlang",
            "emacslisp",
            "k8s",
            "kustomize",
            "llvm_ir",
            "makefile",
            "vimscript",
        ]
        .into_iter()
        .map(|language_id| (language_id, lock.grammar_name_for(language_id).unwrap()))
        .collect::<BTreeMap<_, _>>()
    );
    let symbols = lock
        .grammars
        .iter()
        .map(|grammar| (grammar.name.as_str(), grammar.exported_symbol.as_str()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        BTreeMap::from([
            ("assembly", "tree_sitter_asm"),
            ("cobol", "tree_sitter_COBOL"),
            ("gotemplate", "tree_sitter_gotmpl"),
            ("janet", "tree_sitter_janet_simple"),
            ("php", "tree_sitter_php_only"),
            ("protobuf", "tree_sitter_proto"),
            ("qml", "tree_sitter_qmljs"),
            ("sshconfig", "tree_sitter_ssh_config"),
        ]),
        symbols
            .iter()
            .filter(|(grammar, symbol)| **symbol != format!("tree_sitter_{grammar}"))
            .map(|(grammar, symbol)| (*grammar, *symbol))
            .collect()
    );
    assert_eq!(
        symbols.values().copied().collect::<BTreeSet<_>>().len(),
        159
    );
    assert!(
        lock.language_mappings
            .iter()
            .filter_map(|mapping| mapping.grammar.as_deref())
            .all(|grammar| !grammar.starts_with("objectscript_"))
    );
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
fn lock_requires_exported_symbol() {
    let source = MINIMAL_LOCK.replace("exported_symbol = \"tree_sitter_alpha\"\n", "");

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();
    assert!(error.contains("exported_symbol"), "{error}");
}

#[test]
fn lock_requires_tree_sitter_export_prefix_and_ascii_c_identifier() {
    for invalid_symbol in [
        "alpha",
        "tree_sitter_",
        "tree_sitter_bad-name",
        "tree_sitter_café",
    ] {
        let source = MINIMAL_LOCK.replace("tree_sitter_alpha", invalid_symbol);

        let error = GrammarPackLock::parse(&source).unwrap_err().to_string();

        assert!(
            error.contains("exported symbol") || error.contains("tree_sitter_"),
            "{invalid_symbol}: {error}"
        );
    }
}

#[test]
fn lock_rejects_duplicate_exported_symbols_globally() {
    let second_grammar = r#"
[[grammars]]
name = "beta"
repository = "https://example.invalid/grammar-beta"
commit = "3333333333333333333333333333333333333333"
abi = 15
exported_symbol = "tree_sitter_alpha"
assets = ["LICENSE", "parser.c"]
source_hash = "1111111111111111111111111111111111111111111111111111111111111111"
scanner_language = "c"
license_files = ["LICENSE"]
verdict = "fixture"
provenance_notes = []

"#;
    let second_mapping = r#"
[[language_mappings]]
language_id = "beta"
status = "available"
grammar = "beta"
"#;
    let source = MINIMAL_LOCK
        .replace("declared_grammar_count = 1", "declared_grammar_count = 2")
        .replace(
            "declared_language_binding_count = 1",
            "declared_language_binding_count = 2",
        )
        .replace(
            "[[language_mappings]]",
            &format!("{second_grammar}[[language_mappings]]"),
        )
        + second_mapping;

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();

    assert!(error.contains("duplicate exported symbol"), "{error}");
}

#[test]
fn lock_accepts_only_none_or_c_scanners() {
    let source = MINIMAL_LOCK.replace("scanner_language = \"none\"", "scanner_language = \"cpp\"");

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();

    assert!(error.contains("scanner language"), "{error}");
}

#[test]
fn available_mapping_cannot_target_an_orphan() {
    let source = MINIMAL_LOCK.replace(
        "provenance_notes = []",
        "provenance_notes = []\norphan_reason = \"fixture orphan\"",
    );

    let error = GrammarPackLock::parse(&source).unwrap_err().to_string();

    assert!(error.contains("bound or orphaned"), "{error}");
}
