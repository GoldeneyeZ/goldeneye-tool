use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use goldeneye_grammar_pack::{GrammarPackLock, GrammarPackState, PACK_STATE_FILE, lock_file_hash};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

const ASSET_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-assets-v1\0";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn real_lock() -> GrammarPackLock {
    GrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap()
}

fn real_wrapper_plan() -> Vec<build_script::WrapperSpec> {
    real_lock()
        .grammars
        .iter()
        .enumerate()
        .map(|(ordinal, grammar)| {
            build_script::WrapperSpec::new(
                ordinal,
                &grammar.name,
                &grammar.exported_symbol,
                &grammar.scanner_language,
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn wrapper_plan_is_exact_namespaced_and_deterministic() {
    let lock = real_lock();
    let wrappers = real_wrapper_plan();

    assert_eq!(wrappers.len(), 159);
    assert_eq!(
        wrappers
            .iter()
            .filter(|wrapper| wrapper.has_scanner())
            .count(),
        102
    );
    assert_eq!(
        wrappers
            .iter()
            .filter(|wrapper| !wrapper.has_scanner())
            .count(),
        57
    );
    assert_eq!(
        wrappers
            .iter()
            .map(build_script::WrapperSpec::wrapper_file_name)
            .collect::<BTreeSet<_>>()
            .len(),
        159
    );

    let first = wrappers
        .iter()
        .map(build_script::WrapperSpec::render)
        .collect::<Vec<_>>();
    let second = wrappers
        .iter()
        .map(build_script::WrapperSpec::render)
        .collect::<Vec<_>>();
    assert_eq!(first, second);

    for ((grammar, _wrapper), source) in lock.grammars.iter().zip(&wrappers).zip(&first) {
        let restrict_shim = source.find("#define restrict __restrict").unwrap();
        let parser_include = source
            .find(&format!("#include \"{}/parser.c\"", grammar.name))
            .unwrap();
        assert!(restrict_shim < parser_include);
        assert!(source.contains(&format!(
            "#define {} goldeneye_full_{}",
            grammar.exported_symbol, grammar.exported_symbol
        )));
        assert!(source.contains(&format!("#include \"{}/parser.c\"", grammar.name)));
        assert_eq!(
            source
                .lines()
                .filter(|line| line.starts_with("#include "))
                .count(),
            usize::from(grammar.scanner_language == "c") + usize::from(grammar.name == "cobol") + 1
        );

        for suffix in build_script::STANDARD_SCANNER_SUFFIXES {
            let scanner_symbol = format!("{}_external_scanner_{suffix}", grammar.exported_symbol);
            if grammar.scanner_language == "c" {
                assert!(source.contains(&format!(
                    "#define {scanner_symbol} goldeneye_full_{scanner_symbol}"
                )));
            } else {
                assert!(!source.contains(&scanner_symbol));
            }
        }
    }
}

#[test]
fn helper_sources_are_transitive_not_independent_units() {
    let wrappers = real_wrapper_plan();
    for (grammar, forbidden_helpers) in [
        ("crystal", &["unicode.c"][..]),
        (
            "rst",
            &[
                "tree_sitter_rst/scanner.c",
                "tree_sitter_rst/chars.c",
                "tree_sitter_rst/parser.c",
            ][..],
        ),
        (
            "yaml",
            &["schema.core.c", "schema.json.c", "schema.legacy.c"][..],
        ),
        (
            "vhdl",
            &["TokenTree.inc", "TokenType.inc", "token_tree_match.inc"][..],
        ),
        ("fsharp", &["common/scanner.h"][..]),
        ("qml", &["typescript-scanner.h"][..]),
    ] {
        let wrapper = wrappers
            .iter()
            .find(|wrapper| wrapper.grammar_name() == grammar)
            .unwrap();
        let source = wrapper.render();
        for helper in forbidden_helpers {
            assert!(
                !source.contains(helper),
                "{grammar} helper {helper} became an independent wrapper include"
            );
        }
    }
}

#[test]
fn cfml_resolves_verified_shared_native_headers_through_pack_layout() {
    let wrappers = real_wrapper_plan();
    let cfml = wrappers
        .iter()
        .find(|wrapper| wrapper.grammar_name() == "cfml")
        .unwrap();

    assert_eq!(
        cfml.additional_include_paths(),
        &[PathBuf::from("cfml/tree_sitter")]
    );
    let cobol = wrappers
        .iter()
        .find(|wrapper| wrapper.grammar_name() == "cobol")
        .unwrap();
    assert_eq!(cobol.additional_include_paths(), &[PathBuf::from("cobol")]);
    assert!(
        wrappers
            .iter()
            .filter(|wrapper| !matches!(wrapper.grammar_name(), "cfml" | "cobol"))
            .all(|wrapper| wrapper.additional_include_paths().is_empty())
    );
}

#[test]
fn cobol_wrapper_selects_the_fail_closed_msvc_derived_scanner() {
    let wrappers = real_wrapper_plan();
    let cobol = wrappers
        .iter()
        .find(|wrapper| wrapper.grammar_name() == "cobol")
        .unwrap()
        .render();
    let derived = cobol
        .find("#include \"goldeneye-derived-cobol-scanner.c\"")
        .unwrap();
    let verified = cobol.find("#include \"cobol/scanner.c\"").unwrap();

    assert!(cobol.contains("#ifdef _MSC_VER"));
    assert!(derived < verified);
    assert!(
        wrappers
            .iter()
            .filter(|wrapper| wrapper.grammar_name() != "cobol")
            .all(|wrapper| !wrapper
                .render()
                .contains("goldeneye-derived-cobol-scanner.c"))
    );
}

#[test]
fn cobol_msvc_derivation_changes_only_the_two_proven_vla_bounds() {
    let source = cobol_scanner_fixture();
    let hash = sha256_hex(source.as_bytes());

    let derived = build_script::derive_cobol_scanner_for_msvc(source.as_bytes(), &hash).unwrap();
    let expected = source
        .replacen(
            "char *keyword_pointer[number_of_words];",
            "char *keyword_pointer[9];",
            1,
        )
        .replacen(
            "bool continue_check[number_of_words];",
            "bool continue_check[9];",
            1,
        );

    assert_eq!(derived, expected.as_bytes());
}

#[test]
fn cobol_msvc_derivation_rejects_hash_and_shape_drift() {
    let source = cobol_scanner_fixture();
    let hash_error =
        build_script::derive_cobol_scanner_for_msvc(source.as_bytes(), &"0".repeat(64))
            .unwrap_err();
    assert!(hash_error.contains("SHA-256"));

    let mutated = source.replace(
        "return start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords);",
        "return start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords) ||\n    start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords);",
    );
    let mutation_error = build_script::derive_cobol_scanner_for_msvc(
        mutated.as_bytes(),
        &sha256_hex(mutated.as_bytes()),
    )
    .unwrap_err();
    assert!(mutation_error.contains("exactly one call"));
}

#[test]
fn unsupported_scanner_is_rejected_before_rendering() {
    let error =
        build_script::WrapperSpec::new(0, "unsafe_scanner", "tree_sitter_unsafe_scanner", "cpp")
            .unwrap_err();

    assert!(error.contains("unsupported scanner language"));
}

#[test]
fn missing_cache_remediation_names_the_exact_sync_command() {
    let remediation = build_script::missing_pack_remediation();

    assert!(remediation.contains("GOLDENEYE_GRAMMAR_PACK_DIR"));
    assert!(remediation.contains("cargo xtask grammars sync"));
    assert!(remediation.contains("--git-prefix internal/cbm/vendored/grammars"));
}

#[cfg(windows)]
#[test]
fn msvc_include_path_drops_the_verbatim_prefix() {
    let verbatim = Path::new(r"\\?\D:\verified\grammar-pack");

    assert_eq!(
        build_script::compiler_include_path(verbatim),
        PathBuf::from(r"D:\verified\grammar-pack")
    );
}

#[test]
fn verified_plan_rejects_stale_state_before_wrapper_creation() {
    let fixture = Fixture::new();
    let mut state = fixture.state_value();
    state["lock_hash"] = Value::String("0".repeat(64));
    fs::write(
        fixture.root.join(PACK_STATE_FILE),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();

    let error = fixture.prepare().unwrap_err();

    assert!(error.contains("pack-state.json"));
    assert!(!fixture.wrapper_directory().exists());
}

#[test]
fn verified_plan_rejects_extra_files_before_wrapper_creation() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("unexpected.c"), b"extra\n").unwrap();

    let error = fixture.prepare().unwrap_err();

    assert!(error.contains("file set differs"));
    assert!(!fixture.wrapper_directory().exists());
}

#[test]
fn verified_plan_rejects_hash_drift_before_wrapper_creation() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("alpha/parser.c"),
        b"changed parser fixture\n",
    )
    .unwrap();

    let error = fixture.prepare().unwrap_err();

    assert!(error.contains("hash mismatch"));
    assert!(!fixture.wrapper_directory().exists());
}

#[test]
fn verified_plan_rejects_generated_header_drift() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.generated_path,
        "// goldeneye-full-pack-lock-sha256: 0000000000000000000000000000000000000000000000000000000000000000\n",
    )
    .unwrap();

    let error = fixture.prepare().unwrap_err();

    assert!(error.contains("generated registry lock hash"));
    assert!(!fixture.wrapper_directory().exists());
}

#[test]
fn verified_plan_accepts_an_exact_materialized_fixture_without_writing() {
    let fixture = Fixture::new();

    let plan = fixture.prepare().unwrap();

    assert_eq!(plan.wrappers().len(), 1);
    assert_eq!(plan.asset_paths(), ["alpha/LICENSE", "alpha/parser.c"]);
    assert!(!fixture.wrapper_directory().exists());
}

struct Fixture {
    temporary: TempDir,
    lock_path: PathBuf,
    generated_path: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let lock_path = temporary.path().join("full-pack.toml");
        let generated_path = temporary.path().join("generated.rs");
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
        fs::write(
            root.join(PACK_STATE_FILE),
            serde_json::to_vec_pretty(&GrammarPackState::expected(&lock_path, &lock).unwrap())
                .unwrap(),
        )
        .unwrap();
        fs::write(
            &generated_path,
            format!(
                "// goldeneye-full-pack-lock-sha256: {}\n",
                lock_file_hash(&lock_path).unwrap()
            ),
        )
        .unwrap();

        Self {
            temporary,
            lock_path,
            generated_path,
            root,
        }
    }

    fn prepare(&self) -> Result<build_script::NativePlan, String> {
        build_script::prepare_native_plan(&self.lock_path, &self.generated_path, &self.root)
    }

    fn state_value(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join(PACK_STATE_FILE)).unwrap()).unwrap()
    }

    fn wrapper_directory(&self) -> PathBuf {
        self.temporary.path().join("wrappers")
    }
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

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn cobol_scanner_fixture() -> &'static str {
    r"const int number_of_comment_entry_keywords = 9;
static bool start_with_word( TSLexer *lexer, char *words[], int number_of_words) {
    char *keyword_pointer[number_of_words];
    bool continue_check[number_of_words];
    return keyword_pointer[0] != words[0] || continue_check[0];
}
static bool fixture(TSLexer *lexer) {
    return start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords);
}
"
}

fn tiny_lock(source_hash: &str) -> String {
    format!(
        r#"schema_version = 1
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
repository = "https://example.invalid/alpha"
commit = "2222222222222222222222222222222222222222"
abi = 15
exported_symbol = "tree_sitter_alpha"
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

#[cfg(feature = "compiled")]
mod compiled {
    use std::collections::BTreeSet;
    use std::fs;

    use goldeneye_full_grammars::{
        CompiledGrammar, LookupResult, available_language_count, available_language_ids,
        compiled_source_count, declared_language_count, declared_language_ids, embedded_lock_hash,
        grammar_metadata, lookup, orphan_source_count, unique_grammar_count, upstream_commit,
    };
    use tree_sitter::{Language, Parser};

    fn grammar(id: &str) -> CompiledGrammar {
        match lookup(id).unwrap_or_else(|| panic!("missing declared language {id}")) {
            LookupResult::Available(grammar) => grammar,
            LookupResult::Unavailable { reason } => {
                panic!("expected {id} to be available: {reason}")
            }
        }
    }

    fn language(id: &str) -> Language {
        grammar(id).language_fn().into()
    }

    #[test]
    fn registry_has_exact_safe_static_shape_and_links_every_factory() {
        assert_eq!(declared_language_count(), 160);
        assert_eq!(available_language_count(), 159);
        assert_eq!(unique_grammar_count(), 157);
        assert_eq!(compiled_source_count(), 159);
        assert_eq!(orphan_source_count(), 2);
        assert_eq!(
            upstream_commit(),
            "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c"
        );
        assert_eq!(embedded_lock_hash().len(), 64);

        let declared = declared_language_ids().collect::<Vec<_>>();
        assert_eq!(declared.len(), 160);
        assert!(declared.windows(2).all(|pair| pair[0] < pair[1]));
        let available = available_language_ids().collect::<Vec<_>>();
        assert_eq!(available.len(), 159);
        assert!(available.windows(2).all(|pair| pair[0] < pair[1]));

        let metadata = grammar_metadata().collect::<Vec<_>>();
        assert_eq!(metadata.len(), 157);
        assert!(metadata.windows(2).all(|pair| pair[0].name < pair[1].name));

        let mut unique_names = BTreeSet::new();
        for id in available {
            let grammar = grammar(id);
            let metadata = grammar.metadata();
            unique_names.insert(metadata.name);
            let language: Language = grammar.language_fn().into();
            assert_eq!(
                language.abi_version(),
                usize::try_from(metadata.abi).unwrap()
            );
            let mut parser = Parser::new();
            parser.set_language(&language).unwrap();
            assert!(parser.parse([], None).is_some());
        }
        assert_eq!(unique_names.len(), 157);

        match lookup("nim").unwrap() {
            LookupResult::Unavailable { reason } => {
                assert!(reason.contains("no lang_specs entry or Tree-sitter factory"));
            }
            LookupResult::Available(_) => panic!("nim must remain unavailable"),
        }
        assert!(lookup("objectscript_routine").is_none());
        assert!(lookup("objectscript_udl").is_none());
        assert!(lookup("unknown-language").is_none());
    }

    #[test]
    fn yaml_aliases_share_one_language_function() {
        assert_eq!(language("yaml"), language("k8s"));
        assert_eq!(language("yaml"), language("kustomize"));
    }

    #[test]
    fn core_and_full_factories_link_together_without_symbol_collisions() {
        let core_and_full = [
            (tree_sitter_go::LANGUAGE, "go"),
            (tree_sitter_javascript::LANGUAGE, "javascript"),
            (tree_sitter_python::LANGUAGE, "python"),
            (tree_sitter_rust::LANGUAGE, "rust"),
            (tree_sitter_typescript::LANGUAGE_TYPESCRIPT, "typescript"),
            (tree_sitter_typescript::LANGUAGE_TSX, "tsx"),
        ];

        for (core_fn, id) in core_and_full {
            let core: Language = core_fn.into();
            let full = language(id);
            let mut parser = Parser::new();
            parser.set_language(&core).unwrap();
            parser.set_language(&full).unwrap();
        }
    }

    #[test]
    fn targeted_helper_layout_grammars_parse_nonempty_fixtures() {
        for (id, source) in [
            ("crystal", "puts \"hello\"\n"),
            ("rst", "Title\n=====\n\nParagraph.\n"),
            ("yaml", "name: goldeneye\nitems:\n  - one\n"),
            ("vhdl", "entity demo is\nend entity demo;\n"),
            ("fsharp", "let answer = 42\n"),
            ("qml", "import QtQuick 2.0\nItem { width: 10 }\n"),
        ] {
            let language = language(id);
            let mut parser = Parser::new();
            parser.set_language(&language).unwrap();
            let tree = parser.parse(source, None).unwrap();
            assert!(
                !tree.root_node().has_error(),
                "{id} rejected its targeted helper-layout fixture"
            );
        }
    }

    #[test]
    fn orphan_factories_are_absent_from_the_linked_test_binary() {
        let executable = fs::read(std::env::current_exe().unwrap()).unwrap();
        for orphan in ["routine", "udl"] {
            let symbol = ["goldeneye_full_tree_sitter_", "objectscript_", orphan].concat();
            assert!(
                !executable
                    .windows(symbol.len())
                    .any(|window| window == symbol.as_bytes()),
                "orphan factory was linked: {symbol}"
            );
        }
    }

    #[test]
    fn compiled_grammar_values_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CompiledGrammar>();
    }
}
