#![cfg(feature = "core-grammars")]

use std::{collections::HashSet, sync::Arc};

use goldeneye_domain::{
    ContentHash, FileContext, Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath,
};
use goldeneye_syntax::{
    CoreGrammarProvider, LocatorError, SyntaxEngine, SyntaxSnapshot, all_named_locators,
    locator_scope, resolve_locator,
};
use serde_json::json;

const SOURCE: &[u8] = b"fn alpha(x: i32) -> i32 { x + 1 }";

fn rust_snapshot() -> SyntaxSnapshot {
    SyntaxEngine::new(CoreGrammarProvider)
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(SOURCE),
            Generation::new(7),
        )
        .unwrap()
}

fn file_context(project: &str, path: &str) -> FileContext {
    FileContext::new(
        ProjectId::new(project).unwrap(),
        ProjectRelativePath::new(path).unwrap(),
    )
}

fn locators(snapshot: &SyntaxSnapshot, context: &FileContext) -> Vec<NodeLocator> {
    all_named_locators(snapshot, context).unwrap()
}

fn non_root_locator(snapshot: &SyntaxSnapshot, context: &FileContext) -> NodeLocator {
    locators(snapshot, context)
        .into_iter()
        .find(|locator| !locator.anchor.ancestor_path.is_empty())
        .expect("fixture has a non-root named node")
}

fn field_locator(snapshot: &SyntaxSnapshot, context: &FileContext) -> NodeLocator {
    locators(snapshot, context)
        .into_iter()
        .find(|locator| {
            locator
                .anchor
                .ancestor_path
                .iter()
                .any(|step| step.field_name.is_some())
        })
        .expect("fixture has a named node reached through a field")
}

fn assert_rejected(
    snapshot: &SyntaxSnapshot,
    context: &FileContext,
    locator: &NodeLocator,
    expected: &LocatorError,
) {
    match resolve_locator(snapshot, context, locator) {
        Ok(node) => panic!("malicious locator unexpectedly resolved to {}", node.kind()),
        Err(error) => assert_eq!(&error, expected),
    }
}

#[test]
fn every_named_node_has_unique_resolvable_locator() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let locators = locators(&snapshot, &context);

    assert!(!locators.is_empty());
    let unique: HashSet<_> = locators.iter().collect();
    assert_eq!(unique.len(), locators.len());

    for locator in &locators {
        let node = resolve_locator(&snapshot, &context, locator).unwrap();
        assert!(node.is_named());
        assert_eq!(node.kind(), locator.anchor.node_kind);
        assert_eq!(
            u64::try_from(node.start_byte()).unwrap(),
            locator.anchor.source_span.bytes.start
        );
        assert_eq!(
            u64::try_from(node.end_byte()).unwrap(),
            locator.anchor.source_span.bytes.end
        );
    }
}

#[test]
fn root_locator_uses_empty_ancestor_path() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let root = locators(&snapshot, &context)
        .into_iter()
        .find(|locator| locator.anchor.node_kind == snapshot.root().kind())
        .expect("root locator");

    assert!(root.anchor.ancestor_path.is_empty());
    assert_eq!(
        resolve_locator(&snapshot, &context, &root).unwrap(),
        snapshot.root()
    );
}

#[test]
fn locator_scope_is_derived_from_snapshot_and_current_file_context() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");

    let scope = locator_scope(&snapshot, &context);

    assert_eq!(scope.file, context);
    assert_eq!(&scope.language_id, snapshot.language_id());
    assert_eq!(&scope.grammar, snapshot.grammar());
    assert_eq!(scope.file_hash, snapshot.file_hash());
    assert_eq!(scope.generation, snapshot.generation());
}

#[test]
fn source_derived_locator_has_stable_json_shape_and_round_trips() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let locator = locators(&snapshot, &context)
        .into_iter()
        .find(|locator| {
            let span = locator.anchor.source_span.bytes;
            let start = usize::try_from(span.start).unwrap();
            let end = usize::try_from(span.end).unwrap();
            locator.anchor.node_kind == "identifier" && &snapshot.source()[start..end] == b"alpha"
        })
        .expect("function-name locator");

    let encoded = serde_json::to_value(&locator).unwrap();
    assert_eq!(
        encoded,
        json!({
            "scope": {
                "file": {
                    "project_id": "project",
                    "relative_path": "src/lib.rs"
                },
                "language_id": "rust",
                "grammar": {
                    "provider": "rust-crate",
                    "grammar": "rust",
                    "revision": "tree-sitter-rust@0.24.2",
                    "abi": snapshot.grammar().abi
                },
                "file_hash": ContentHash::of(SOURCE).to_string(),
                "generation": 7
            },
            "anchor": {
                "ancestor_path": [
                    {
                        "node_kind": "function_item",
                        "named_child_index": 0,
                        "field_name": null
                    },
                    {
                        "node_kind": "identifier",
                        "named_child_index": 0,
                        "field_name": "name"
                    }
                ],
                "node_kind": "identifier",
                "source_span": {
                    "bytes": { "start": 3, "end": 8 },
                    "start": { "row": 0, "column_bytes": 3 },
                    "end": { "row": 0, "column_bytes": 8 }
                },
                "content_hash": ContentHash::of(b"alpha").to_string()
            }
        })
    );

    let decoded: NodeLocator = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, locator);
    assert_eq!(
        resolve_locator(&snapshot, &context, &decoded)
            .unwrap()
            .kind(),
        "identifier"
    );
}

#[test]
fn wrong_locator_project_is_rejected_first() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.file.project_id = ProjectId::new("other").unwrap();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::ProjectMismatch,
    );
}

#[test]
fn wrong_locator_path_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.file.relative_path = ProjectRelativePath::new("src/other.rs").unwrap();

    assert_rejected(&snapshot, &context, &locator, &LocatorError::PathMismatch);
}

#[test]
fn wrong_requested_language_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.language_id = LanguageId::new("python").unwrap();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::LanguageMismatch,
    );
}

#[test]
fn wrong_grammar_provider_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.grammar.provider = "malicious-provider".to_owned();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::GrammarProviderMismatch,
    );
}

#[test]
fn wrong_grammar_name_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.grammar.grammar = "python".to_owned();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::GrammarNameMismatch,
    );
}

#[test]
fn wrong_grammar_revision_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.grammar.revision = "untrusted@0.0.0".to_owned();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::GrammarRevisionMismatch,
    );
}

#[test]
fn wrong_grammar_abi_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.grammar.abi = locator.scope.grammar.abi.saturating_add(1);

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::GrammarAbiMismatch,
    );
}

#[test]
fn wrong_file_hash_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.file_hash = ContentHash::of(b"not the source");

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::FileHashMismatch,
    );
}

#[test]
fn wrong_generation_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.generation = Generation::new(8);

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::GenerationMismatch,
    );
}

#[test]
fn invalid_ancestor_named_index_is_rejected_without_fallback() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.ancestor_path[0].named_child_index = u32::MAX;

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::AncestorIndexOutOfBounds { depth: 0 },
    );
}

#[test]
fn wrong_ancestor_kind_is_rejected_without_byte_fallback() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.ancestor_path[0].node_kind = "malicious_kind".to_owned();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::AncestorKindMismatch { depth: 0 },
    );
}

#[test]
fn wrong_ancestor_field_is_rejected_after_raw_child_recovery() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = field_locator(&snapshot, &context);
    let depth = locator
        .anchor
        .ancestor_path
        .iter()
        .position(|step| step.field_name.is_some())
        .unwrap();
    locator.anchor.ancestor_path[depth].field_name = Some("malicious_field".to_owned());

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::AncestorFieldMismatch { depth },
    );
}

#[test]
fn wrong_terminal_kind_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.node_kind = "malicious_kind".to_owned();

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::TerminalKindMismatch,
    );
}

#[test]
fn wrong_terminal_byte_range_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.source_span.bytes.end = locator.anchor.source_span.bytes.end.saturating_add(1);

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::TerminalByteRangeMismatch,
    );
}

#[test]
fn wrong_terminal_point_span_is_rejected() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.source_span.start.column_bytes = locator
        .anchor
        .source_span
        .start
        .column_bytes
        .saturating_add(1);

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::TerminalPointSpanMismatch,
    );
}

#[test]
fn wrong_terminal_content_hash_is_rejected_without_fuzzy_matching() {
    let snapshot = rust_snapshot();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.anchor.content_hash = ContentHash::of(b"different node bytes");

    assert_rejected(
        &snapshot,
        &context,
        &locator,
        &LocatorError::TerminalContentHashMismatch,
    );
}

#[test]
fn typed_errors_do_not_embed_raw_source() {
    let snapshot = SyntaxEngine::new(CoreGrammarProvider)
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(&b"fn secret_token_83d291() {}"[..]),
            Generation::new(1),
        )
        .unwrap();
    let context = file_context("project", "src/lib.rs");
    let mut locator = non_root_locator(&snapshot, &context);
    locator.scope.file_hash = ContentHash::of(b"wrong");

    let error = resolve_locator(&snapshot, &context, &locator).unwrap_err();
    assert!(!error.to_string().contains("secret_token_83d291"));
}
