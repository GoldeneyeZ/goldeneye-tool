#![cfg(feature = "full-grammar-tests")]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::num::NonZeroUsize;

use goldeneye_discovery::{DiscoveryOptions, IndexMode};
use goldeneye_domain::{FileId, LanguageId, ProjectRelativePath};
use goldeneye_index::{CancellationToken, IndexOptions, IndexService, IndexStatus};
use goldeneye_store::Store;
use goldeneye_syntax::{FullGrammarProvider, GrammarProvider};
use tempfile::TempDir;

#[path = "support/full_language_fixtures.rs"]
mod full_language_fixtures;

use full_language_fixtures::LANGUAGE_FIXTURES;

struct HybridFixture {
    language: &'static str,
    target_path: &'static str,
    target_source: &'static str,
    caller_path: &'static str,
    caller_source: &'static str,
    target_name: &'static str,
}

const HYBRID_FIXTURES: &[HybridFixture] = &[
    HybridFixture {
        language: "go",
        target_path: "go_pkg/target.go",
        target_source: "package target\n\nfunc GoTarget() int { return 1 }\n",
        caller_path: "main.go",
        caller_source: "package main\n\nimport target \"./go_pkg\"\n\nfunc GoCaller() int { return target.GoTarget() }\n",
        target_name: "GoTarget",
    },
    HybridFixture {
        language: "c",
        target_path: "c_target.c",
        target_source: "int c_target(void) { return 1; }\n",
        caller_path: "c_caller.c",
        caller_source: "#include \"c_target.h\"\nint c_caller(void) { return c_target(); }\n",
        target_name: "c_target",
    },
    HybridFixture {
        language: "cpp",
        target_path: "cpp_target.cpp",
        target_source: "int cpp_target() { return 1; }\n",
        caller_path: "cpp_caller.cpp",
        caller_source: "#include \"cpp_target.hpp\"\nint cpp_caller() { return cpp_target(); }\n",
        target_name: "cpp_target",
    },
    HybridFixture {
        language: "cuda",
        target_path: "cuda_target.cu",
        target_source: "__device__ int cuda_target() { return 1; }\n",
        caller_path: "cuda_caller.cu",
        caller_source: "#include \"cuda_target.cuh\"\n__device__ int cuda_caller() { return cuda_target(); }\n",
        target_name: "cuda_target",
    },
    HybridFixture {
        language: "python",
        target_path: "py_target.py",
        target_source: "def py_target():\n    return 1\n",
        caller_path: "py_caller.py",
        caller_source: "from py_target import py_target as py_alias\n\ndef py_caller():\n    return py_alias()\n",
        target_name: "py_target",
    },
    HybridFixture {
        language: "javascript",
        target_path: "js_target.js",
        target_source: "export function jsTarget() { return 1; }\n",
        caller_path: "js_caller.js",
        caller_source: "import { jsTarget as jsAlias } from './js_target.js';\nexport function jsCaller() { return jsAlias(); }\n",
        target_name: "jsTarget",
    },
    HybridFixture {
        language: "typescript",
        target_path: "ts_target.ts",
        target_source: "export function tsTarget(): number { return 1; }\n",
        caller_path: "ts_caller.ts",
        caller_source: "import { tsTarget as tsAlias } from './ts_target';\nexport function tsCaller(): number { return tsAlias(); }\n",
        target_name: "tsTarget",
    },
    HybridFixture {
        language: "tsx",
        target_path: "tsx_target.tsx",
        target_source: "export function tsxTarget(): number { return 1; }\n",
        caller_path: "tsx_caller.tsx",
        caller_source: "import { tsxTarget as tsxAlias } from './tsx_target';\nexport function TsxCaller(): number { return tsxAlias(); }\n",
        target_name: "tsxTarget",
    },
    HybridFixture {
        language: "php",
        target_path: "php_target.php",
        target_source: "<?php\nnamespace Demo;\nfunction phpTarget() { return 1; }\n",
        caller_path: "php_caller.php",
        caller_source: "<?php\nuse function Demo\\phpTarget as phpAlias;\nfunction phpCaller() { return phpAlias(); }\n",
        target_name: "phpTarget",
    },
    HybridFixture {
        language: "csharp",
        target_path: "CsTarget.cs",
        target_source: "namespace Demo { public static class CsTarget { public static int Run() { return 1; } } }\n",
        caller_path: "CsCaller.cs",
        caller_source: "using Alias = Demo.CsTarget;\npublic class CsCaller { public int Call() { return Alias.Run(); } }\n",
        target_name: "Run",
    },
    HybridFixture {
        language: "java",
        target_path: "demo/JavaTarget.java",
        target_source: "package demo; public class JavaTarget { public static int javaTarget() { return 1; } }\n",
        caller_path: "demo/JavaCaller.java",
        caller_source: "package demo; import static demo.JavaTarget.javaTarget; public class JavaCaller { public int call() { return javaTarget(); } }\n",
        target_name: "javaTarget",
    },
    HybridFixture {
        language: "kotlin",
        target_path: "demo/KotlinTarget.kt",
        target_source: "package demo\nfun kotlinTarget(): Int = 1\n",
        caller_path: "demo/KotlinCaller.kt",
        caller_source: "package demo\nimport demo.kotlinTarget as kotlinAlias\nfun kotlinCaller(): Int = kotlinAlias()\n",
        target_name: "kotlinTarget",
    },
    HybridFixture {
        language: "rust",
        target_path: "rust_target.rs",
        target_source: "pub fn rust_target() -> i32 { 1 }\n",
        caller_path: "lib.rs",
        caller_source: "mod rust_target;\nuse rust_target::rust_target as rust_alias;\npub fn rust_caller() -> i32 { rust_alias() }\n",
        target_name: "rust_target",
    },
];

fn full_options() -> IndexOptions {
    IndexOptions {
        discovery: DiscoveryOptions {
            mode: IndexMode::Full,
            ..DiscoveryOptions::default()
        },
        max_workers: NonZeroUsize::new(1).expect("one worker"),
        max_files: None,
        cancellation: CancellationToken::new(),
        project_id_override: None,
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
fn audited_hybrid_lsp_languages_resolve_cross_file_calls() {
    assert_eq!(HYBRID_FIXTURES.len(), 13);
    for fixture in HYBRID_FIXTURES {
        let temp = TempDir::new().expect("hybrid fixture repository");
        let target = temp.path().join(fixture.target_path);
        let caller = temp.path().join(fixture.caller_path);
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
        fs::create_dir_all(caller.parent().expect("caller parent")).expect("create caller parent");
        fs::write(&target, fixture.target_source).expect("write hybrid target");
        fs::write(&caller, fixture.caller_source).expect("write hybrid caller");

        let mut service = IndexService::new(
            Store::open_in_memory().expect("memory store"),
            FullGrammarProvider,
            full_options(),
        );
        let result = service
            .index_repository(temp.path())
            .unwrap_or_else(|error| {
                panic!(
                    "{} hybrid fixture failed to index: {error}",
                    fixture.language
                )
            });
        let target_file = FileId::new(
            result.project.id.clone(),
            ProjectRelativePath::new(fixture.target_path).expect("target path"),
        );
        let caller_file = FileId::new(
            result.project.id.clone(),
            ProjectRelativePath::new(fixture.caller_path).expect("caller path"),
        );
        let target_ids = service
            .store()
            .nodes_for_file(&target_file)
            .expect("target nodes")
            .into_iter()
            .filter(|node| node.name == fixture.target_name)
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        assert!(
            !target_ids.is_empty(),
            "{} fixture has no target definition named {}",
            fixture.language,
            fixture.target_name
        );
        let calls = service
            .store()
            .nodes_for_file(&caller_file)
            .expect("caller nodes")
            .into_iter()
            .flat_map(|node| {
                service
                    .store()
                    .edges_from(&result.project.id, &node.id)
                    .expect("caller edges")
            })
            .filter(|edge| edge.kind.as_str() == "CALLS" && target_ids.contains(&edge.target))
            .collect::<Vec<_>>();
        assert_eq!(
            calls.len(),
            1,
            "{} fixture did not resolve exactly one cross-file target: {calls:#?}",
            fixture.language
        );
        assert!(
            calls[0]
                .properties
                .get("strategy")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|strategy| strategy.starts_with("hybrid_")),
            "{} fixture did not use hybrid resolution",
            fixture.language
        );
        let unchanged = service
            .index_repository(temp.path())
            .expect("repeat hybrid index");
        assert_eq!(unchanged.status, IndexStatus::Unchanged);
        assert_eq!(unchanged.project.generation, result.project.generation);
    }
}

#[test]
fn hybrid_relations_resolve_cross_file_inheritance_and_interfaces() {
    let temp = TempDir::new().expect("hybrid relation repository");
    fs::write(temp.path().join("base.py"), "class Base:\n    pass\n").expect("write Python base");
    fs::write(
        temp.path().join("child.py"),
        "from base import Base\nclass Child(Base):\n    pass\n",
    )
    .expect("write Python child");
    fs::create_dir_all(temp.path().join("demo")).expect("create Java package");
    fs::write(
        temp.path().join("demo/Contract.java"),
        "package demo; public interface Contract {}\n",
    )
    .expect("write Java interface");
    fs::write(
        temp.path().join("demo/Implementation.java"),
        "package demo; import demo.Contract; public class Implementation implements Contract {}\n",
    )
    .expect("write Java implementation");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        FullGrammarProvider,
        full_options(),
    );
    let result = service
        .index_repository(temp.path())
        .expect("index relation fixtures");
    let mut resolved = BTreeSet::new();
    for node in service
        .store()
        .list_nodes(&result.project.id)
        .expect("project nodes")
    {
        for edge in service
            .store()
            .edges_from(&result.project.id, &node.id)
            .expect("relation edges")
        {
            if matches!(edge.kind.as_str(), "INHERITS" | "IMPLEMENTS") {
                let target = service
                    .store()
                    .get_node(&result.project.id, &edge.target)
                    .expect("target lookup")
                    .expect("relation target");
                resolved.insert((edge.kind.as_str().to_owned(), target.name));
            }
        }
    }
    assert!(resolved.contains(&("INHERITS".to_owned(), "Base".to_owned())));
    assert!(resolved.contains(&("IMPLEMENTS".to_owned(), "Contract".to_owned())));
}

#[test]
#[allow(clippy::too_many_lines)]
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
