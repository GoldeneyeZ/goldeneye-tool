use std::sync::Arc;

use goldeneye_domain::{
    ContentHash, FileContext, Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath,
};
use goldeneye_edit::{EditOperation, EditOptions, plan_edit};
use goldeneye_syntax::{
    CoreGrammarProvider, SyntaxEngine, SyntaxSnapshot, all_named_locators, resolve_locator,
};

fn context(path: &str) -> FileContext {
    FileContext::new(
        ProjectId::new("goldeneye").unwrap(),
        ProjectRelativePath::new(path).unwrap(),
    )
}

fn snapshot(language: &str, source: &[u8]) -> (SyntaxEngine<CoreGrammarProvider>, SyntaxSnapshot) {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            LanguageId::new(language).unwrap(),
            Arc::<[u8]>::from(source),
            Generation::new(1),
        )
        .unwrap();
    (engine, snapshot)
}

fn nth_locator(
    snapshot: &SyntaxSnapshot,
    file: &FileContext,
    kind: &str,
    index: usize,
) -> NodeLocator {
    all_named_locators(snapshot, file)
        .unwrap()
        .into_iter()
        .filter(|locator| locator.anchor.node_kind == kind)
        .nth(index)
        .unwrap_or_else(|| panic!("fixture has no {kind} at index {index}"))
}

#[test]
fn replacement_parses_cleanly_across_core_agent_languages() {
    let cases = [
        (
            "rust",
            "src/lib.rs",
            "fn old() { 1 }\n",
            "function_item",
            "fn changed() { 2 }",
            "fn changed() { 2 }\n",
        ),
        (
            "python",
            "pkg/app.py",
            "def old():\n    return 1\n",
            "function_definition",
            "def changed():\n    return 2",
            "def changed():\n    return 2\n",
        ),
        (
            "javascript",
            "src/app.js",
            "function old() { return 1; }\n",
            "function_declaration",
            "function changed() { return 2; }",
            "function changed() { return 2; }\n",
        ),
        (
            "typescript",
            "src/app.ts",
            "function old(): number { return 1; }\n",
            "function_declaration",
            "function changed(): number { return 2; }",
            "function changed(): number { return 2; }\n",
        ),
        (
            "go",
            "main.go",
            "package main\nfunc old() int { return 1 }\n",
            "function_declaration",
            "func changed() int { return 2 }",
            "package main\nfunc changed() int { return 2 }\n",
        ),
    ];

    for (language, path, source, kind, replacement, expected) in cases {
        let file = context(path);
        let (engine, snapshot) = snapshot(language, source.as_bytes());
        let locator = nth_locator(&snapshot, &file, kind, 0);
        let plan = plan_edit(
            &engine,
            &snapshot,
            &file,
            &locator,
            &EditOperation::Replace(replacement.to_owned()),
            Generation::new(2),
            &EditOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));

        assert_eq!(plan.source.as_ref(), expected.as_bytes(), "{language}");
        assert!(!plan.snapshot.has_errors(), "{language}");
        assert_eq!(plan.diagnostics.after_total, 0, "{language}");
        assert_eq!(plan.new_file_hash, ContentHash::of(expected.as_bytes()));
        for refreshed in &plan.refreshed_locators {
            resolve_locator(&plan.snapshot, &file, refreshed).unwrap();
        }
    }
}

#[test]
fn delete_and_adjacent_insertions_change_only_one_named_node_boundary() {
    let source = b"fn first() {}\nfn keep() {}";
    let file = context("src/lib.rs");
    let cases = [
        (EditOperation::Delete, "\nfn keep() {}"),
        (
            EditOperation::InsertBefore("fn before() {}\n".to_owned()),
            "fn before() {}\nfn first() {}\nfn keep() {}",
        ),
        (
            EditOperation::InsertAfter("\nfn after() {}".to_owned()),
            "fn first() {}\nfn after() {}\nfn keep() {}",
        ),
    ];

    for (operation, expected) in cases {
        let (engine, snapshot) = snapshot("rust", source);
        let locator = nth_locator(&snapshot, &file, "function_item", 0);
        let plan = plan_edit(
            &engine,
            &snapshot,
            &file,
            &locator,
            &operation,
            Generation::new(2),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.source.as_ref(), expected.as_bytes());
        assert!(!plan.snapshot.has_errors());
    }
}

#[test]
fn root_insertions_preserve_original_bytes_on_each_side() {
    let source = b"fn keep() {}";
    let file = context("src/lib.rs");
    let cases = [
        (
            EditOperation::InsertBefore("// before\n".to_owned()),
            "// before\nfn keep() {}",
        ),
        (
            EditOperation::InsertAfter("\n// after".to_owned()),
            "fn keep() {}\n// after",
        ),
    ];

    for (operation, expected) in cases {
        let (engine, snapshot) = snapshot("rust", source);
        let root = nth_locator(&snapshot, &file, "source_file", 0);
        let plan = plan_edit(
            &engine,
            &snapshot,
            &file,
            &root,
            &operation,
            Generation::new(2),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.source.as_ref(), expected.as_bytes());
        assert!(!plan.snapshot.has_errors());
    }
}

#[test]
fn duplicate_identical_nodes_use_path_identity_not_text_search() {
    let source = b"fn same() {}\nfn same() {}";
    let file = context("src/lib.rs");
    let (engine, snapshot) = snapshot("rust", source);
    let second = nth_locator(&snapshot, &file, "function_item", 1);

    let plan = plan_edit(
        &engine,
        &snapshot,
        &file,
        &second,
        &EditOperation::Replace("fn changed() {}".to_owned()),
        Generation::new(2),
        &EditOptions::default(),
    )
    .unwrap();

    assert_eq!(plan.source.as_ref(), b"fn same() {}\nfn changed() {}");
}

#[test]
fn unicode_offsets_and_minimal_diff_stay_on_utf8_boundaries() {
    let source = "const S: &str = \"é\";\nfn keep() { println!(\"🙂\"); }";
    let expected = "const S: &str = \"ê\";\nfn keep() { println!(\"🙂\"); }";
    let file = context("src/lib.rs");
    let (engine, snapshot) = snapshot("rust", source.as_bytes());
    let string = nth_locator(&snapshot, &file, "string_literal", 0);

    let plan = plan_edit(
        &engine,
        &snapshot,
        &file,
        &string,
        &EditOperation::Replace("\"ê\"".to_owned()),
        Generation::new(2),
        &EditOptions::default(),
    )
    .unwrap();

    assert_eq!(plan.source.as_ref(), expected.as_bytes());
    let old_start = usize::try_from(plan.diff.old_span.start).unwrap();
    let old_end = usize::try_from(plan.diff.old_span.end).unwrap();
    let new_start = usize::try_from(plan.diff.new_span.start).unwrap();
    let new_end = usize::try_from(plan.diff.new_span.end).unwrap();
    assert_eq!(&source[old_start..old_end], "é");
    assert_eq!(
        std::str::from_utf8(&plan.source[new_start..new_end]).unwrap(),
        "ê"
    );
    assert_eq!(
        plan.diff.removed_hash,
        ContentHash::of(&source.as_bytes()[old_start..old_end])
    );
    assert_eq!(
        plan.diff.inserted_hash,
        ContentHash::of(&plan.source[new_start..new_end])
    );
}
