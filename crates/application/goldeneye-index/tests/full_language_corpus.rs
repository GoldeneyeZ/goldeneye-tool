#![cfg(feature = "full-grammar-tests")]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::num::NonZeroUsize;

use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::{FileId, LanguageId, ProjectRelativePath};
use goldeneye_index::{
    CancellationToken, IndexMode, IndexOptions, IndexService, IndexStatus,
    RepositoryDiscoveryOptions,
};
use goldeneye_store::Store;
use goldeneye_syntax::{FullGrammarProvider, GrammarProvider};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
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
        discovery: RepositoryDiscoveryOptions {
            mode: IndexMode::Full,
            ..RepositoryDiscoveryOptions::default()
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
fn same_stem_properties_and_xml_files_keep_distinct_file_identity() {
    let temp = TempDir::new().expect("temp repository");
    let paths = [
        "spring-context/src/test/resources/org/springframework/context/annotation/configuration/AutowiredConfigurationTests-custom.properties",
        "spring-context/src/test/resources/org/springframework/context/annotation/configuration/AutowiredConfigurationTests-custom.xml",
    ];
    fs::create_dir_all(temp.path().join(paths[0]).parent().expect("fixture parent"))
        .expect("create fixture parent");
    fs::write(temp.path().join(paths[0]), "name=value\n").expect("write properties fixture");
    fs::write(
        temp.path().join(paths[1]),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<beans/>\n",
    )
    .expect("write XML fixture");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("replace project graph");

    assert_eq!(result.counts.files, 2);
    let mut file_ids = BTreeSet::new();
    let mut qualified_names = BTreeSet::new();
    for path in paths {
        let file = FileId::new(
            result.project.id.clone(),
            ProjectRelativePath::new(path).expect("fixture path"),
        );
        let file_node = service
            .repository()
            .nodes_for_file(&file)
            .expect("file nodes")
            .into_iter()
            .find(|node| node.label.as_str() == "File")
            .expect("File node");
        assert_eq!(
            file_node
                .file_path
                .as_ref()
                .map(ProjectRelativePath::as_str),
            Some(path)
        );
        file_ids.insert(file_node.id);
        qualified_names.insert(file_node.qualified_name);
    }
    assert_eq!(file_ids.len(), 2);
    assert_eq!(qualified_names.len(), 2);
}

#[test]
fn cpp_header_and_source_can_repeat_a_private_definition() {
    let temp = TempDir::new().expect("temp repository");
    let source = r#"
struct OverrideUI {};
struct SuppressUI: public OverrideUI {
    SuppressUI(): OverrideUI() {}
};
"#;
    fs::write(temp.path().join("MsiUtils.cpp"), source).expect("write C++ source");
    fs::write(temp.path().join("MsiUtils.h"), source).expect("write C++ header");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index repeated C++ definition");

    assert_eq!(result.counts.files, 2);
    let suppress_ui = service
        .repository()
        .search_nodes(&result.project.id, "SuppressUI", 10)
        .expect("search repeated definition")
        .into_iter()
        .filter(|hit| hit.node.label.as_str() == "Class")
        .collect::<Vec<_>>();
    assert_eq!(suppress_ui.len(), 1);
}

#[test]
fn cpp_nested_constructor_definitions_match_header_declarations() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("ResourceEditor.h"),
        concat!(
            "class ResourceEditor { public:\n",
            "  class FileLock { public:\n",
            "    explicit FileLock(int);\n",
            "    explicit FileLock(long);\n",
            "  };\n",
            "};\n",
        ),
    )
    .expect("write C++ header");
    fs::write(
        temp.path().join("ResourceEditor.cpp"),
        concat!(
            "ResourceEditor::FileLock::FileLock(int) {}\n",
            "ResourceEditor::FileLock::FileLock(long) {}\n",
        ),
    )
    .expect("write C++ source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index nested C++ constructors");

    let hits = service
        .repository()
        .search_nodes(&result.project.id, "FileLock", 10)
        .expect("search nested class and constructors");
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.node.label.as_str() == "Class")
            .count(),
        1
    );
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.node.label.as_str() == "Method")
            .count(),
        2
    );
}

#[test]
fn cpp_anonymous_namespace_does_not_steal_a_descendant_name() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(temp.path().join("CfgFile.h"), "class CfgFile {};\n").expect("write C++ header");
    fs::write(
        temp.path().join("CfgFile.cpp"),
        concat!(
            "namespace {\n",
            "template <typename CfgFile> void helper(CfgFile&) {}\n",
            "}\n",
        ),
    )
    .expect("write C++ source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index anonymous C++ namespace");

    let hits = service
        .repository()
        .search_nodes(&result.project.id, "CfgFile", 10)
        .expect("search CfgFile nodes");
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.node.label.as_str() == "Class")
            .count(),
        1
    );
    assert!(!hits.iter().any(|hit| {
        hit.node.label.as_str() == "Module"
            && hit
                .node
                .qualified_name
                .as_str()
                .ends_with(".CfgFile.CfgFile")
    }));
}

#[test]
fn javascript_function_values_take_the_name_they_are_assigned_to() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("panel.ts"),
        concat!(
            "export const add = (a: number, b: number) => a + b;\n",
            "export function useThing() {\n",
            "  useEffect(() => {\n",
            "    native.start();\n",
            "  }, []);\n",
            "  const stop = () => native.stop();\n",
            "  return { run: async () => native.run(), stop };\n",
            "}\n",
            "class Box {\n",
            "  open = () => native.open();\n",
            "}\n",
            "target.handler = function () { return native.handle(); };\n",
        ),
    )
    .expect("write TypeScript source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index TypeScript function values");

    let symbols = |name: &str| -> Vec<(String, String)> {
        service
            .repository()
            .search_nodes(&result.project.id, name, 20)
            .expect("search nodes")
            .into_iter()
            .filter(|hit| hit.node.name == name)
            .map(|hit| {
                (
                    hit.node.label.as_str().to_owned(),
                    hit.node.qualified_name.as_str().to_owned(),
                )
            })
            .collect()
    };

    // A callback is not a symbol, and nothing is named after what a body mentions first.
    for stolen in ["native", "a"] {
        assert_eq!(symbols(stolen), Vec::new(), "{stolen}");
    }
    // One symbol per declared function, carrying the declared name.
    for (name, label, owner) in [
        ("add", "Function", ".panel.add"),
        ("stop", "Function", ".useThing.stop"),
        ("run", "Function", ".useThing.run"),
        ("open", "Method", ".Box.open"),
        ("handler", "Function", ".panel.handler"),
    ] {
        let found = symbols(name);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert_eq!(found[0].0, label, "{name}");
        assert!(found[0].1.ends_with(owner), "{name}: {}", found[0].1);
    }
}

#[test]
fn lambdas_and_closures_are_not_symbols_and_their_calls_belong_to_the_function_around_them() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("Config.java"),
        concat!(
            "class Config {\n",
            "  Chain chain(Http http) {\n",
            "    return http.csrf(csrf -> csrf.disable()).authorize(() -> new Policy()).build();\n",
            "  }\n",
            "}\n",
            "class Policy {}\n",
        ),
    )
    .expect("write Java source");
    fs::write(
        temp.path().join("lib.rs"),
        concat!(
            "pub struct ApiError;\n",
            "pub fn convert(input: Option<u8>) -> Result<u8, ApiError> {\n",
            "    input.map(|value| value + 1).ok_or_else(|| ApiError)\n",
            "}\n",
        ),
    )
    .expect("write Rust source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index lambdas and closures");

    let labels = |name: &str| -> Vec<String> {
        let mut found: Vec<String> = service
            .repository()
            .search_nodes(&result.project.id, name, 20)
            .expect("search nodes")
            .into_iter()
            .filter(|hit| hit.node.name == name)
            .map(|hit| hit.node.label.as_str().to_owned())
            .collect();
        found.sort();
        found
    };

    // Nothing is named after a lambda's parameter or after what its body mentions.
    assert_eq!(labels("csrf"), Vec::<String>::new());
    assert_eq!(labels("value"), Vec::<String>::new());
    // A type that a lambda instantiates is that type and nothing else.
    assert_eq!(labels("Policy"), vec!["Class".to_owned()]);
    assert_eq!(labels("ApiError"), vec!["Struct".to_owned()]);
    // The functions around them are still there.
    assert_eq!(labels("chain"), vec!["Method".to_owned()]);
    assert_eq!(labels("convert"), vec!["Function".to_owned()]);
}

#[test]
fn a_java_package_line_is_not_a_class() {
    let temp = TempDir::new().expect("temp repository");
    fs::create_dir_all(temp.path().join("org/demo")).expect("package directory");
    fs::write(
        temp.path().join("org/demo/Ledger.java"),
        concat!(
            "package org.demo;\n",
            "\n",
            "import org.demo.util.Clock;\n",
            "\n",
            "public class Ledger {\n",
            "  long stamp() { return Clock.now(); }\n",
            "}\n",
        ),
    )
    .expect("write Java source");
    fs::create_dir_all(temp.path().join("org/demo/util")).expect("package directory");
    fs::write(
        temp.path().join("org/demo/util/Clock.java"),
        "package org.demo.util;\n\npublic class Clock {\n  public static long now() { return 0L; }\n}\n",
    )
    .expect("write Java source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index Java packages");

    let labels = |name: &str| -> Vec<String> {
        let mut found: Vec<String> = service
            .repository()
            .search_nodes(&result.project.id, name, 20)
            .expect("search nodes")
            .into_iter()
            .filter(|hit| hit.node.name == name)
            .map(|hit| hit.node.label.as_str().to_owned())
            // A file is a module named after its stem; that is not at issue here.
            .filter(|label| !matches!(label.as_str(), "File" | "Module"))
            .collect();
        found.sort();
        found
    };

    // The first segment of the package name is no symbol.
    assert_eq!(labels("org"), Vec::<String>::new());
    // The types and the call between the two files are as before.
    assert_eq!(labels("Ledger"), vec!["Class".to_owned()]);
    assert_eq!(labels("Clock"), vec!["Class".to_owned()]);
    let calls = service
        .repository()
        .search_nodes(&result.project.id, "stamp", 20)
        .expect("search nodes")
        .into_iter()
        .filter(|hit| hit.node.name == "stamp")
        .flat_map(|hit| {
            service
                .repository()
                .edges_from(&result.project.id, &hit.node.id)
                .expect("edges of stamp")
        })
        .filter(|edge| edge.kind.as_str() == "CALLS")
        .count();
    assert_eq!(calls, 1, "Ledger.stamp calls Clock.now");
}

#[test]
fn a_type_is_one_symbol_and_an_unnamed_one_is_none() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("store.go"),
        concat!(
            "package store\n",
            "\n",
            "type Ledger struct {\n",
            "\tTotal int\n",
            "}\n",
            "\n",
            "type (\n",
            "\tReader interface {\n",
            "\t\tRead() int\n",
            "\t}\n",
            "\tCents int\n",
            ")\n",
            "\n",
            "func (ledger *Ledger) Read() int { return ledger.Total }\n",
            "\n",
            "var defaultLedger = Ledger{}\n",
            "\n",
            "const (\n",
            "\tPhaseScan = iota\n",
            "\tPhaseWrite\n",
            ")\n",
        ),
    )
    .expect("write Go source");
    fs::write(
        temp.path().join("page.c"),
        concat!(
            "typedef struct {\n",
            "    int capacity;\n",
            "    int used;\n",
            "} PageBuilder;\n",
            "\n",
            "struct Tagged;\n",
            "typedef struct Tagged tagged_t;\n",
            "\n",
            "struct Tagged {\n",
            "    int value;\n",
            "};\n",
            "\n",
            "typedef enum { RISK_LOW, RISK_HIGH } risk_level_t;\n",
            "\n",
            "int page_used(PageBuilder *page) { return page->used; }\n",
            "int tagged_value(void) { struct Tagged local = {0}; return local.value; }\n",
        ),
    )
    .expect("write C source");
    fs::write(
        temp.path().join("mock.ts"),
        concat!(
            "export const mocks = {\n",
            "  Observer: class {\n",
            "    observe(): void {}\n",
            "  },\n",
            "};\n",
            "export class Named {\n",
            "  run(): void {}\n",
            "}\n",
        ),
    )
    .expect("write TypeScript source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service.index_repository(temp.path()).expect("index types");

    let labels = |name: &str| -> Vec<String> {
        let mut found: Vec<String> = service
            .repository()
            .search_nodes(&result.project.id, name, 20)
            .expect("search nodes")
            .into_iter()
            .filter(|hit| hit.node.name == name)
            .map(|hit| hit.node.label.as_str().to_owned())
            .filter(|label| !matches!(label.as_str(), "File" | "Module"))
            .collect();
        found.sort();
        found
    };

    // Go: one symbol per type, also in a grouped declaration.
    assert_eq!(labels("Ledger"), vec!["Struct".to_owned()]);
    assert_eq!(labels("Reader"), vec!["Interface".to_owned()]);
    assert_eq!(labels("Cents"), vec!["Type".to_owned()]);
    // Go: one symbol per variable, and one per constant of a group.
    assert_eq!(labels("defaultLedger"), vec!["Variable".to_owned()]);
    assert_eq!(labels("PhaseScan"), vec!["Variable".to_owned()]);
    assert_eq!(labels("PhaseWrite"), vec!["Variable".to_owned()]);
    // C: an untagged struct is not named after its first field; an untagged
    // enum is still found by its first enumerator.
    assert_eq!(labels("capacity"), vec!["Field".to_owned()]);
    assert_eq!(labels("RISK_LOW"), vec!["Enum".to_owned()]);
    assert_eq!(labels("PageBuilder"), vec!["Type".to_owned()]);
    assert_eq!(labels("risk_level_t"), vec!["Type".to_owned()]);
    // C: the struct is where its body is; announcing or using it declares nothing.
    assert_eq!(labels("Tagged"), vec!["Class".to_owned()]);
    assert_eq!(labels("tagged_t"), vec!["Type".to_owned()]);
    // TypeScript: a class expression without a name is not called after its first method.
    assert_eq!(labels("observe"), vec!["Method".to_owned()]);
    assert_eq!(labels("Named"), vec!["Class".to_owned()]);
}

#[test]
fn a_makefile_rule_is_one_function_named_by_its_target() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("Makefile"),
        concat!(
            "DIST_DIR := dist\n",
            "\n",
            "package: build\n",
            "\tmkdir -p $(DIST_DIR)/bin\n",
            "\tcp app $(DIST_DIR)/bin\n",
            "\n",
            "build:\n",
            "\tgo build ./...\n",
        ),
    )
    .expect("write Makefile");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index Makefile");

    let labels = |name: &str| -> Vec<String> {
        let mut found: Vec<String> = service
            .repository()
            .search_nodes(&result.project.id, name, 20)
            .expect("search nodes")
            .into_iter()
            .filter(|hit| hit.node.name == name)
            .map(|hit| hit.node.label.as_str().to_owned())
            .collect();
        found.sort();
        found
    };

    assert_eq!(labels("package"), vec!["Function".to_owned()]);
    assert_eq!(labels("build"), vec!["Function".to_owned()]);
    // The variable the commands use is the variable, not a function.
    assert_eq!(labels("DIST_DIR"), vec!["Variable".to_owned()]);
}

#[test]
fn a_powershell_variable_is_a_symbol_where_it_is_assigned_only() {
    let temp = TempDir::new().expect("temp repository");
    fs::write(
        temp.path().join("install.ps1"),
        concat!(
            "$Repo = \"owner/name\"\n",
            "Write-Host \"https://example.test/$Repo/releases\"\n",
            "Write-Host $Repo\n",
        ),
    )
    .expect("write PowerShell source");

    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index PowerShell");

    let named: Vec<String> = service
        .repository()
        .search_nodes(&result.project.id, "Repo", 20)
        .expect("search nodes")
        .into_iter()
        .filter(|hit| hit.node.name.trim_start_matches('$') == "Repo")
        .map(|hit| hit.node.label.as_str().to_owned())
        .collect();
    assert_eq!(named, vec!["Variable".to_owned()]);
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
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index Elixir fixture");
    let file = FileId::new(
        result.project.id.clone(),
        ProjectRelativePath::new("demo.ex").expect("fixture path"),
    );
    let nodes = service
        .repository()
        .nodes_for_file(&file)
        .expect("Elixir nodes");
    let definitions = nodes
        .iter()
        .map(|node| (node.label.as_str(), node.name.as_str()))
        .collect::<Vec<_>>();
    assert!(definitions.contains(&("Function", "run")));
    assert!(definitions.contains(&("Function", "helper")));

    let has_call = nodes.iter().any(|node| {
        service
            .repository()
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
            TreeSitterIndexExtractor::new(FullGrammarProvider),
            full_options(),
            FileSystemDiscovery,
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
            .repository()
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
            .repository()
            .nodes_for_file(&caller_file)
            .expect("caller nodes")
            .into_iter()
            .flat_map(|node| {
                service
                    .repository()
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
        TreeSitterIndexExtractor::new(FullGrammarProvider),
        full_options(),
        FileSystemDiscovery,
    );
    let result = service
        .index_repository(temp.path())
        .expect("index relation fixtures");
    let mut resolved = BTreeSet::new();
    for node in service
        .repository()
        .list_nodes(&result.project.id)
        .expect("project nodes")
    {
        for edge in service
            .repository()
            .edges_from(&result.project.id, &node.id)
            .expect("relation edges")
        {
            if matches!(edge.kind.as_str(), "INHERITS" | "IMPLEMENTS") {
                let target = service
                    .repository()
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
            TreeSitterIndexExtractor::new(FullGrammarProvider),
            corpus_options(fixture.language),
            FileSystemDiscovery,
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
            .repository()
            .nodes_for_file(&file)
            .unwrap_or_else(|error| panic!("{} corpus nodes failed: {error}", fixture.language));
        if !nodes.iter().any(|node| node.label.as_str() == "File") {
            failures.push(format!("{}: missing File node", fixture.language));
        }
        let actual_labels = nodes
            .iter()
            .map(|node| node.label.as_str())
            .collect::<BTreeSet<_>>();
        for expected in fixture
            .expected_labels
            .iter()
            .copied()
            .filter(|label| *label != "Import")
        {
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
                .repository()
                .edges_from(&result.project.id, &node.id)
                .unwrap_or_else(|error| panic!("{} corpus edges failed: {error}", fixture.language))
            {
                match edge.kind.as_str() {
                    "CALLS" => {
                        call_edges += 1;
                    }
                    "INHERITS" | "IMPLEMENTS" => inheritance_edges += 1,
                    _ => {}
                }
            }
        }
    }

    println!(
        "159-language extraction: labels={label_counts:?}, resolved_calls={call_edges}, expected_raw_calls={expected_raw_calls}, expected_import_fixtures={expected_import_fixtures}, inheritance={inheritance_edges}, expected_relations={expected_relations}"
    );
    println!("missing expected labels by category: {missing_labels:?}");
    assert!(failures.is_empty(), "corpus failures: {failures:#?}");
    assert!(
        missing_labels.is_empty(),
        "missing expected labels: {missing_labels:#?}"
    );
    assert!(label_counts.get("Function").copied().unwrap_or_default() > 0);
    assert!(call_edges > 0);
    assert!(!label_counts.contains_key("Import"));
    assert!(expected_relations > 0);
    assert!(
        inheritance_edges > 0,
        "expected at least one same-file inheritance/interface edge"
    );
}
