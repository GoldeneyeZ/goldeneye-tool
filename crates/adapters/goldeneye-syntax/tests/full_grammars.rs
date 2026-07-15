#![cfg(feature = "full-grammar-pack")]

use std::collections::BTreeSet;
use std::sync::Arc;

use goldeneye_domain::LanguageId;
use goldeneye_full_grammars::{
    CompiledGrammar, LookupResult, available_language_count, available_language_ids,
    declared_language_count, declared_language_ids, grammar_metadata, lookup, orphan_source_count,
    unique_grammar_count,
};
use goldeneye_syntax::{FullGrammarProvider, GrammarProvider, GrammarSource, SyntaxError};
use tree_sitter::Parser;

const LANGUAGE_GRAMMAR_EXCEPTIONS: [(&str, &str); 8] = [
    ("csharp", "c_sharp"),
    ("dlang", "d"),
    ("emacslisp", "elisp"),
    ("k8s", "yaml"),
    ("kustomize", "yaml"),
    ("llvm_ir", "llvm"),
    ("makefile", "make"),
    ("vimscript", "vim"),
];

const GRAMMAR_FACTORY_EXCEPTIONS: [(&str, &str); 8] = [
    ("assembly", "tree_sitter_asm"),
    ("cobol", "tree_sitter_COBOL"),
    ("gotemplate", "tree_sitter_gotmpl"),
    ("janet", "tree_sitter_janet_simple"),
    ("php", "tree_sitter_php_only"),
    ("protobuf", "tree_sitter_proto"),
    ("qml", "tree_sitter_qmljs"),
    ("sshconfig", "tree_sitter_ssh_config"),
];

fn language_id(value: &str) -> LanguageId {
    LanguageId::new(value).expect("test language IDs are non-empty")
}

fn compiled_grammar(language_id: &str) -> CompiledGrammar {
    match lookup(language_id).unwrap_or_else(|| panic!("missing declared language {language_id}")) {
        LookupResult::Available(grammar) => grammar,
        LookupResult::Unavailable { reason } => {
            panic!("expected {language_id} to be available: {reason}")
        }
    }
}

#[test]
fn full_provider_preserves_exact_registry_and_exception_contracts() {
    let provider = FullGrammarProvider;
    assert_eq!(declared_language_count(), 160);
    assert_eq!(available_language_count(), 159);
    assert_eq!(unique_grammar_count(), 157);
    assert_eq!(orphan_source_count(), 2);

    let declared = declared_language_ids().collect::<Vec<_>>();
    let available = available_language_ids().collect::<Vec<_>>();
    let supported = provider.supported_ids();
    assert_eq!(declared.len(), 160);
    assert_eq!(available.len(), 159);
    assert_eq!(supported.len(), 159);
    assert!(declared.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(available.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(supported.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        supported.iter().map(LanguageId::as_str).collect::<Vec<_>>(),
        available
    );
    assert_eq!(
        declared
            .iter()
            .copied()
            .filter(|id| !available.contains(id))
            .collect::<Vec<_>>(),
        ["nim"]
    );

    let language_exceptions = available
        .iter()
        .filter_map(|id| {
            let grammar = compiled_grammar(id).metadata();
            (grammar.name != *id).then_some((*id, grammar.name))
        })
        .collect::<Vec<_>>();
    assert_eq!(language_exceptions, LANGUAGE_GRAMMAR_EXCEPTIONS);

    let factory_exceptions = grammar_metadata()
        .filter_map(|grammar| {
            let conventional = format!("tree_sitter_{}", grammar.name);
            (grammar.exported_symbol != conventional)
                .then_some((grammar.name, grammar.exported_symbol))
        })
        .collect::<Vec<_>>();
    assert_eq!(factory_exceptions, GRAMMAR_FACTORY_EXCEPTIONS);
}

#[test]
fn unavailable_unknown_and_orphan_ids_have_exact_typed_failures() {
    let provider = FullGrammarProvider;
    for value in [
        "nim",
        "unknown-language",
        "objectscript_routine",
        "objectscript_udl",
    ] {
        let id = language_id(value);
        assert_eq!(
            provider.grammar(&id),
            Err(SyntaxError::UnsupportedGrammar {
                language_id: id.clone(),
            })
        );
        assert!(!provider.supported_ids().contains(&id));
    }

    match lookup("nim") {
        Some(LookupResult::Unavailable { reason }) => assert_eq!(
            reason,
            "codebase-memory-mcp retains the language ID but has no lang_specs entry or Tree-sitter factory at the pinned commit"
        ),
        Some(LookupResult::Available(_)) | None => panic!("nim must be the one unavailable ID"),
    }
    for orphan in ["objectscript_routine", "objectscript_udl"] {
        assert!(lookup(orphan).is_none());
        assert!(!declared_language_ids().any(|id| id == orphan));
        assert!(!available_language_ids().any(|id| id == orphan));
        assert!(!grammar_metadata().any(|metadata| metadata.name.contains("objectscript")));
    }

    let mismatch = SyntaxError::GrammarAbiMismatch {
        language_id: language_id("rust"),
        expected: 14,
        actual: 15,
    };
    assert_eq!(
        mismatch.to_string(),
        "Tree-sitter grammar ABI mismatch for language LanguageId(\"rust\"): expected 14, got 15"
    );
}

#[test]
fn every_supported_id_has_locked_provenance_and_a_viable_parser_lifecycle() {
    let provider = FullGrammarProvider;
    let mut unique_grammars = BTreeSet::new();

    for id in provider.supported_ids() {
        let expected = compiled_grammar(id.as_str()).metadata();
        let grammar = provider.grammar(&id).expect("supported grammar must load");
        assert_eq!(grammar.language_id, id);
        assert_eq!(grammar.abi, expected.abi);
        assert!((13..=15).contains(&grammar.abi));
        assert_eq!(
            grammar.source,
            GrammarSource::FullPack {
                grammar: expected.name.into(),
                source_hash: expected.source_hash.into(),
            }
        );
        assert_eq!(
            u32::try_from(grammar.language.abi_version()).unwrap(),
            expected.abi
        );
        unique_grammars.insert(expected.name);

        let mut parser = Parser::new();
        parser
            .set_language(&grammar.language)
            .expect("locked ABI must be accepted by Tree-sitter");
        assert!(parser.parse([], None).is_some());
        drop(parser);
        drop(grammar);
    }

    assert_eq!(unique_grammars.len(), 157);
}

#[test]
fn yaml_aliases_and_scanner_fixtures_parse_nonempty_source() {
    let provider = FullGrammarProvider;
    let language = |id| provider.grammar(&language_id(id)).unwrap().language;
    assert_eq!(language("yaml"), language("k8s"));
    assert_eq!(language("yaml"), language("kustomize"));

    for (id, source) in [
        ("crystal", "puts \"hello\"\n"),
        ("rst", "Title\n=====\n\nParagraph.\n"),
        ("yaml", "name: goldeneye\nitems:\n  - one\n"),
        ("vhdl", "entity demo is\nend entity demo;\n"),
        ("fsharp", "let answer = 42\n"),
        ("qml", "import QtQuick 2.0\nItem { width: 10 }\n"),
        (
            "purescript",
            "module Main where\n\nanswer :: Int\nanswer = 42\n",
        ),
        ("rescript", "let answer = 42\n"),
    ] {
        let grammar = provider.grammar(&language_id(id)).unwrap();
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{id} rejected its non-empty scanner fixture"
        );
    }
}

#[test]
fn full_provider_is_send_sync_and_supports_concurrent_lookup() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FullGrammarProvider>();

    let provider = Arc::new(FullGrammarProvider);
    let expected_ids = provider.supported_ids();
    let threads = (0..4)
        .map(|_| {
            let provider = Arc::clone(&provider);
            let expected_ids = expected_ids.clone();
            std::thread::spawn(move || {
                for id in expected_ids {
                    let grammar = provider.grammar(&id).unwrap();
                    assert_eq!(grammar.language_id, id);
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().expect("concurrent lookup must not panic");
    }
}

#[cfg(feature = "core-grammars")]
#[test]
fn core_and_full_providers_link_and_run_in_one_binary() {
    use goldeneye_syntax::CoreGrammarProvider;

    let core = CoreGrammarProvider;
    let full = FullGrammarProvider;
    let rust = language_id("rust");
    let yaml = language_id("yaml");
    let k8s = language_id("k8s");
    let kustomize = language_id("kustomize");

    let core_rust = core.grammar(&rust).unwrap();
    let full_rust = full.grammar(&rust).unwrap();
    let full_yaml = full.grammar(&yaml).unwrap();
    let full_k8s = full.grammar(&k8s).unwrap();
    let full_kustomize = full.grammar(&kustomize).unwrap();

    let mut parser = Parser::new();
    for language in [
        &core_rust.language,
        &full_rust.language,
        &full_yaml.language,
        &full_k8s.language,
        &full_kustomize.language,
    ] {
        parser.set_language(language).unwrap();
        assert!(parser.parse([], None).is_some());
    }
}
