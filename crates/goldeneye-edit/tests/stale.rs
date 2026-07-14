use std::sync::Arc;

use goldeneye_domain::{
    ContentHash, FileContext, Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath,
    SourcePoint,
};
use goldeneye_edit::{EditError, EditOperation, EditOptions, plan_edit};
use goldeneye_syntax::{
    CoreGrammarProvider, LocatorError, SyntaxEngine, SyntaxSnapshot, all_named_locators,
};

fn context() -> FileContext {
    FileContext::new(
        ProjectId::new("goldeneye").unwrap(),
        ProjectRelativePath::new("src/lib.rs").unwrap(),
    )
}

fn snapshot(source: &[u8], generation: u64) -> SyntaxSnapshot {
    SyntaxEngine::new(CoreGrammarProvider)
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(source),
            Generation::new(generation),
        )
        .unwrap()
}

fn locator(snapshot: &SyntaxSnapshot, index: usize) -> NodeLocator {
    all_named_locators(snapshot, &context())
        .unwrap()
        .into_iter()
        .filter(|locator| locator.anchor.node_kind == "function_item")
        .nth(index)
        .unwrap()
}

fn stale_cause(snapshot: &SyntaxSnapshot, locator: &NodeLocator) -> LocatorError {
    let result = plan_edit(
        &SyntaxEngine::new(CoreGrammarProvider),
        snapshot,
        &context(),
        locator,
        &EditOperation::Delete,
        Generation::new(snapshot.generation().value() + 1),
        &EditOptions::default(),
    );
    match result {
        Err(EditError::StaleLocator { cause, fresh }) => {
            assert_eq!(fresh.scope.file_hash, snapshot.file_hash());
            assert_eq!(fresh.scope.generation, snapshot.generation());
            assert_eq!(fresh.scope.file, context());
            cause
        }
        Err(other) => panic!("unexpected edit error: {other}"),
        Ok(_) => panic!("tampered locator unexpectedly planned an edit"),
    }
}

#[test]
fn every_scope_guard_returns_its_typed_stale_cause() {
    let snapshot = snapshot(b"fn same() {}\nfn same() {}", 7);
    let original = locator(&snapshot, 0);

    let mut changed = original.clone();
    changed.scope.file.project_id = ProjectId::new("other").unwrap();
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::ProjectMismatch
    );

    let mut changed = original.clone();
    changed.scope.file.relative_path = ProjectRelativePath::new("src/other.rs").unwrap();
    assert_eq!(stale_cause(&snapshot, &changed), LocatorError::PathMismatch);

    let mut changed = original.clone();
    changed.scope.language_id = LanguageId::new("python").unwrap();
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::LanguageMismatch
    );

    let mut changed = original.clone();
    changed.scope.grammar.provider.push_str("-other");
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::GrammarProviderMismatch
    );

    let mut changed = original.clone();
    changed.scope.grammar.grammar.push_str("-other");
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::GrammarNameMismatch
    );

    let mut changed = original.clone();
    changed.scope.grammar.revision.push_str("-other");
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::GrammarRevisionMismatch
    );

    let mut changed = original.clone();
    changed.scope.grammar.abi += 1;
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::GrammarAbiMismatch
    );

    let mut changed = original.clone();
    changed.scope.file_hash = ContentHash::of(b"different");
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::FileHashMismatch
    );

    let mut changed = original;
    changed.scope.generation = Generation::new(6);
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::GenerationMismatch
    );
}

#[test]
fn every_anchor_guard_is_required_even_for_identical_node_text() {
    let snapshot = snapshot(b"fn same() {}\nfn same() {}", 7);
    let original = locator(&snapshot, 0);
    let second = locator(&snapshot, 1);

    let mut changed = original.clone();
    changed.anchor.ancestor_path[0].named_child_index = 99;
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::AncestorIndexOutOfBounds { depth: 0 }
    );

    let mut changed = original.clone();
    changed.anchor.ancestor_path[0].node_kind = "struct_item".to_owned();
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::AncestorKindMismatch { depth: 0 }
    );

    let mut changed = original.clone();
    changed.anchor.ancestor_path[0].field_name = Some("fake".to_owned());
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::AncestorFieldMismatch { depth: 0 }
    );

    let mut changed = original.clone();
    changed.anchor.node_kind = "struct_item".to_owned();
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::TerminalKindMismatch
    );

    let mut changed = original.clone();
    changed.anchor.source_span.bytes = second.anchor.source_span.bytes;
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::TerminalByteRangeMismatch
    );

    let mut changed = original.clone();
    changed.anchor.source_span.start = SourcePoint::new(99, 0);
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::TerminalPointSpanMismatch
    );

    let mut changed = original;
    changed.anchor.content_hash = ContentHash::of(b"different");
    assert_eq!(
        stale_cause(&snapshot, &changed),
        LocatorError::TerminalContentHashMismatch
    );
}

#[test]
fn shifted_range_is_only_a_hint_and_never_fuzzy_relocates() {
    let old = snapshot(b"fn same() {}", 7);
    let mut old_locator = locator(&old, 0);
    let shifted = snapshot(b"\nfn same() {}", 8);
    old_locator.scope.file_hash = shifted.file_hash();
    old_locator.scope.generation = shifted.generation();

    assert_eq!(
        stale_cause(&shifted, &old_locator),
        LocatorError::TerminalByteRangeMismatch
    );
}
