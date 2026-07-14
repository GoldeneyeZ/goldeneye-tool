#![cfg(feature = "full-grammar-tests")]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::num::NonZeroUsize;

use goldeneye_discovery::{DiscoveryOptions, IndexMode};
use goldeneye_domain::{FileId, LanguageId, ProjectRelativePath};
use goldeneye_index::{CancellationToken, IndexOptions, IndexService};
use goldeneye_store::Store;
use goldeneye_syntax::{FullGrammarProvider, GrammarProvider};
use tempfile::TempDir;

#[path = "support/full_language_fixtures.rs"]
mod full_language_fixtures;

use full_language_fixtures::LANGUAGE_FIXTURES;

fn full_options() -> IndexOptions {
    IndexOptions {
        discovery: DiscoveryOptions {
            mode: IndexMode::Full,
            ..DiscoveryOptions::default()
        },
        max_workers: NonZeroUsize::new(1).expect("one worker"),
        max_files: None,
        cancellation: CancellationToken::new(),
    }
}

fn corpus_path(language: &str, upstream_path: &str) -> String {
    if language == "sshconfig" {
        "config.sshconfig".to_owned()
    } else {
        upstream_path.to_owned()
    }
}

fn corpus_options(language: &str) -> IndexOptions {
    let mut options = full_options();
    let extension = match language {
        "gitignore" => Some(".gitignore"),
        "jsdoc" => Some(".jsdoc"),
        "k8s" => Some(".yaml"),
        "nasm" => Some(".asm"),
        "objc" => Some(".m"),
        "sshconfig" => Some(".sshconfig"),
        _ => None,
    };
    if let Some(extension) = extension {
        options.discovery.extension_overrides.insert(
            OsString::from(extension),
            LanguageId::new(language).expect("fixture language ID"),
        );
    }
    options
}

#[test]
fn elixir_definition_calls_use_audited_language_rules() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("demo.ex"),
        "defmodule Demo do\n  def run(), do: helper()\n  def helper(), do: :ok\nend\n",
    )
    .expect("write Elixir fixture");
    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        FullGrammarProvider,
        full_options(),
    );
    let result = service
        .index_repository(temp.path())
        .expect("index Elixir fixture");
    let file = FileId::new(
        result.project.id.clone(),
        ProjectRelativePath::new("demo.ex").expect("fixture path"),
    );
    let nodes = service.store().nodes_for_file(&file).expect("Elixir nodes");
    let definitions = nodes
        .iter()
        .map(|node| (node.label.as_str(), node.name.as_str()))
        .collect::<Vec<_>>();
    assert!(definitions.contains(&("Function", "run")));
    assert!(definitions.contains(&("Function", "helper")));

    let has_call = nodes.iter().any(|node| {
        service
            .store()
            .edges_from(&result.project.id, &node.id)
            .expect("Elixir edges")
            .iter()
            .any(|edge| edge.kind.as_str() == "CALLS")
    });
    assert!(has_call);
}

#[test]
fn audited_159_language_corpus_is_callable_and_indexable() {
    assert_eq!(LANGUAGE_FIXTURES.len(), 159);
    let supported = FullGrammarProvider
        .supported_ids()
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let fixture_ids = LANGUAGE_FIXTURES
        .iter()
        .map(|fixture| fixture.language.to_owned())
        .collect::<Vec<_>>();
    assert_eq!(fixture_ids, supported);

    let temp = TempDir::new().expect("corpus repository");
    let mut failures = Vec::new();
    let mut label_counts = BTreeMap::<String, usize>::new();
    let mut call_edges = 0usize;
    let mut import_edges = 0usize;
    let mut inheritance_edges = 0usize;
    let mut missing_labels = BTreeMap::<String, Vec<String>>::new();
    let expected_raw_calls = LANGUAGE_FIXTURES
        .iter()
        .filter(|fixture| fixture.callee.is_some())
        .count();
    let expected_import_fixtures = LANGUAGE_FIXTURES
        .iter()
        .filter(|fixture| fixture.expects_import)
        .count();
    let expected_relations = LANGUAGE_FIXTURES
        .iter()
        .map(|fixture| fixture.expected_inherits.len() + fixture.expected_implements.len())
        .sum::<usize>();

    for fixture in LANGUAGE_FIXTURES {
        let root = temp.path().join(fixture.language);
        let relative_path = corpus_path(fixture.language, fixture.path);
        let file_path = root.join(&relative_path);
        fs::create_dir_all(file_path.parent().expect("fixture parent"))
            .expect("create fixture directory");
        fs::write(&file_path, fixture.source).expect("write corpus fixture");

        let mut service = IndexService::new(
            Store::open_in_memory().expect("memory store"),
            FullGrammarProvider,
            corpus_options(fixture.language),
        );
        let result = service
            .index_repository(&root)
            .unwrap_or_else(|error| panic!("{} corpus index failed: {error}", fixture.language));
        if result.parsed_files != 1 {
            failures.push(format!(
                "{}: parsed_files={}",
                fixture.language, result.parsed_files
            ));
            continue;
        }
        let file = FileId::new(
            result.project.id.clone(),
            ProjectRelativePath::new(&relative_path).expect("fixture path"),
        );
        let nodes = service
            .store()
            .nodes_for_file(&file)
            .unwrap_or_else(|error| panic!("{} corpus nodes failed: {error}", fixture.language));
        if !nodes.iter().any(|node| node.label.as_str() == "File") {
            failures.push(format!("{}: missing File node", fixture.language));
        }
        let actual_labels = nodes
            .iter()
            .map(|node| node.label.as_str())
            .collect::<BTreeSet<_>>();
        for expected in fixture.expected_labels {
            if !actual_labels.contains(expected) {
                missing_labels
                    .entry((*expected).to_owned())
                    .or_default()
                    .push(fixture.language.to_owned());
            }
        }
        for node in &nodes {
            *label_counts
                .entry(node.label.as_str().to_owned())
                .or_default() += 1;
            for edge in service
                .store()
                .edges_from(&result.project.id, &node.id)
                .unwrap_or_else(|error| panic!("{} corpus edges failed: {error}", fixture.language))
            {
                match edge.kind.as_str() {
                    "CALLS" => {
                        call_edges += 1;
                    }
                    "IMPORTS" => import_edges += 1,
                    "INHERITS" | "IMPLEMENTS" => inheritance_edges += 1,
                    _ => {}
                }
            }
        }
    }

    println!(
        "159-language extraction: labels={label_counts:?}, resolved_calls={call_edges}, expected_raw_calls={expected_raw_calls}, imports={import_edges}, expected_import_fixtures={expected_import_fixtures}, inheritance={inheritance_edges}, expected_relations={expected_relations}"
    );
    println!("missing expected labels by category: {missing_labels:?}");
    assert!(failures.is_empty(), "corpus failures: {failures:#?}");
    assert!(
        missing_labels.is_empty(),
        "missing expected labels: {missing_labels:#?}"
    );
    assert!(label_counts.get("Function").copied().unwrap_or_default() > 0);
    assert!(call_edges > 0);
    assert!(
        import_edges >= expected_import_fixtures,
        "expected at least one import edge for every import-positive fixture"
    );
    assert!(expected_relations > 0);
    assert!(
        inheritance_edges > 0,
        "expected at least one same-file inheritance/interface edge"
    );
}
