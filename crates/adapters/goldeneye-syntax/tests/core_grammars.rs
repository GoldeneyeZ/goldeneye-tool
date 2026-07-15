#![cfg(feature = "core-grammars")]

use goldeneye_domain::LanguageId;
use goldeneye_syntax::{CoreGrammarProvider, GrammarProvider, GrammarSource, SyntaxError};
use std::collections::BTreeMap;

const CORE_GRAMMAR_PACKAGES: [&str; 5] = [
    "tree-sitter-go",
    "tree-sitter-javascript",
    "tree-sitter-python",
    "tree-sitter-rust",
    "tree-sitter-typescript",
];

const CORE_RUNTIME_PACKAGES: [(&str, &str); 6] = [
    ("go", "tree-sitter-go"),
    ("javascript", "tree-sitter-javascript"),
    ("python", "tree-sitter-python"),
    ("rust", "tree-sitter-rust"),
    ("tsx", "tree-sitter-typescript"),
    ("typescript", "tree-sitter-typescript"),
];

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

fn exact_manifest_pins(manifest: &str) -> Result<BTreeMap<String, String>, String> {
    let document = toml::from_str::<toml::Value>(manifest)
        .map_err(|error| format!("invalid goldeneye-syntax manifest: {error}"))?;
    let dependencies = document
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "manifest is missing [dependencies]".to_owned())?;

    CORE_GRAMMAR_PACKAGES
        .into_iter()
        .map(|package| {
            let dependency = dependencies
                .get(package)
                .and_then(toml::Value::as_table)
                .ok_or_else(|| format!("{package} must be an inline dependency table"))?;
            if dependency.get("optional").and_then(toml::Value::as_bool) != Some(true) {
                return Err(format!("{package} must be optional"));
            }
            let requirement = dependency
                .get("version")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| format!("{package} must declare a version"))?;
            let version = requirement
                .strip_prefix('=')
                .filter(|version| !version.is_empty())
                .ok_or_else(|| format!("{package} must use an exact =version pin"))?;

            Ok((package.to_owned(), version.to_owned()))
        })
        .collect()
}

fn validate_provider_metadata_against_manifest(manifest: &str) -> Result<(), String> {
    let pins = exact_manifest_pins(manifest)?;

    for (id, expected_package) in CORE_RUNTIME_PACKAGES {
        let grammar = CoreGrammarProvider
            .grammar(&LanguageId::new(id).expect("core runtime IDs are non-empty"))
            .map_err(|error| format!("provider rejected {id}: {error}"))?;
        let GrammarSource::RustCrate { package, version } = grammar.source else {
            return Err(format!("{id} did not report Rust crate provenance"));
        };

        if package != expected_package {
            return Err(format!(
                "{id} reported package {package}, expected {expected_package}"
            ));
        }

        let pinned_version = pins
            .get(expected_package)
            .expect("every runtime package has a checked manifest pin");
        if version != *pinned_version {
            return Err(format!(
                "{expected_package} manifest pin {pinned_version} disagrees with {id} provider metadata {version}"
            ));
        }
    }

    Ok(())
}

fn manifest_with_drifted_pin(manifest: &str, package: &str) -> String {
    let mut document = toml::from_str::<toml::Value>(manifest).unwrap();
    let dependencies = document
        .get_mut("dependencies")
        .and_then(toml::Value::as_table_mut)
        .unwrap();
    let dependency = dependencies
        .get(package)
        .and_then(toml::Value::as_table)
        .unwrap();
    let requirement = dependency
        .get("version")
        .and_then(toml::Value::as_str)
        .unwrap();
    let drifted = format!("{requirement}-synthetic-drift");
    dependencies
        .get_mut(package)
        .and_then(toml::Value::as_table_mut)
        .unwrap()
        .insert("version".into(), toml::Value::String(drifted));

    toml::to_string(&document).unwrap()
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
fn provider_reports_abi_value_semantics_and_typed_unsupported_error() {
    for (id, _) in CORE_RUNTIME_PACKAGES {
        let grammar = CoreGrammarProvider
            .grammar(&LanguageId::new(id).unwrap())
            .unwrap();

        assert_eq!(grammar.language_id.as_str(), id);
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

#[test]
fn provider_metadata_matches_exact_manifest_pins() {
    validate_provider_metadata_against_manifest(include_str!("../Cargo.toml")).unwrap();
}

#[test]
fn metadata_guard_rejects_each_synthetic_manifest_pin_drift() {
    let manifest = include_str!("../Cargo.toml");

    for package in CORE_GRAMMAR_PACKAGES {
        let drifted_manifest = manifest_with_drifted_pin(manifest, package);
        let error = validate_provider_metadata_against_manifest(&drifted_manifest).unwrap_err();

        assert!(
            error.contains(package)
                && error.contains("manifest pin")
                && error.contains("provider metadata"),
            "drift failure for {package} was not a provider metadata mismatch: {error}"
        );
    }
}
