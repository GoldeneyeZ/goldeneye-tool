use goldeneye_domain::LanguageId;
use goldeneye_syntax::{CoreGrammarProvider, GrammarProvider, GrammarSource, SyntaxError};

struct Fixture {
    id: &'static str,
    source: &'static str,
    root_kind: &'static str,
}

fn fixtures() -> [Fixture; 6] {
    [
        Fixture {
            id: "go",
            source: "package main\nfunc main() {}\n",
            root_kind: "source_file",
        },
        Fixture {
            id: "javascript",
            source: "function greet() {}\n",
            root_kind: "program",
        },
        Fixture {
            id: "python",
            source: "def greet():\n    pass\n",
            root_kind: "module",
        },
        Fixture {
            id: "rust",
            source: "fn greet() {}\n",
            root_kind: "source_file",
        },
        Fixture {
            id: "tsx",
            source: "const view = <div>Hello</div>;\n",
            root_kind: "program",
        },
        Fixture {
            id: "typescript",
            source: "interface Thing { value: number }\n",
            root_kind: "program",
        },
    ]
}

fn expected_core_metadata() -> [(&'static str, &'static str, &'static str); 6] {
    [
        ("go", "tree-sitter-go", "0.25.0"),
        ("javascript", "tree-sitter-javascript", "0.25.0"),
        ("python", "tree-sitter-python", "0.25.0"),
        ("rust", "tree-sitter-rust", "0.24.2"),
        ("tsx", "tree-sitter-typescript", "0.23.2"),
        ("typescript", "tree-sitter-typescript", "0.23.2"),
    ]
}

#[test]
fn core_provider_exposes_exact_language_set() {
    let provider = CoreGrammarProvider;

    assert_eq!(
        provider
            .supported_ids()
            .iter()
            .map(LanguageId::as_str)
            .collect::<Vec<_>>(),
        ["go", "javascript", "python", "rust", "tsx", "typescript"]
    );
}

#[test]
fn every_core_grammar_parses_valid_source() {
    for fixture in fixtures() {
        let grammar = CoreGrammarProvider
            .grammar(&LanguageId::new(fixture.id).unwrap())
            .unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&grammar.language).unwrap();
        let tree = parser.parse(fixture.source.as_bytes(), None).unwrap();

        assert_eq!(tree.root_node().kind(), fixture.root_kind);
        assert!(!tree.root_node().has_error());
        let abi = usize::try_from(grammar.abi).unwrap();
        assert!(
            (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
                .contains(&abi)
        );
        assert!(grammar.language.node_kind_count() > 0);
    }
}

#[test]
fn provider_reports_pinned_metadata_and_typed_unsupported_error() {
    for (id, package, version) in expected_core_metadata() {
        let grammar = CoreGrammarProvider
            .grammar(&LanguageId::new(id).unwrap())
            .unwrap();

        assert_eq!(grammar.language_id.as_str(), id);
        assert_eq!(
            grammar.source,
            GrammarSource::RustCrate {
                package: package.into(),
                version: version.into(),
            }
        );
        assert_eq!(
            usize::try_from(grammar.abi).unwrap(),
            grammar.language.abi_version()
        );
        assert_eq!(grammar.clone(), grammar);
    }

    let full_pack_source = GrammarSource::FullPack {
        grammar: "rust".into(),
        source_hash: "abc123".into(),
    };
    assert_eq!(full_pack_source.clone(), full_pack_source);
    assert!(format!("{full_pack_source:?}").contains("FullPack"));

    let unsupported = LanguageId::new("java").unwrap();
    assert!(matches!(
        CoreGrammarProvider.grammar(&unsupported),
        Err(SyntaxError::UnsupportedGrammar { language_id })
            if language_id == unsupported
    ));
}

#[test]
fn core_provider_is_safe_to_share_between_workers() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<CoreGrammarProvider>();
}
