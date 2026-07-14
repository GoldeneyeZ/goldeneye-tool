use std::sync::Arc;

use goldeneye_domain::{FileContext, Generation, LanguageId, ProjectId, ProjectRelativePath};
use goldeneye_edit::{
    EditError, EditOperation, EditOptions, ParsePolicy, plan_edit, validate_create_content,
};
use goldeneye_syntax::{CoreGrammarProvider, LocatorError, SyntaxEngine, all_named_locators};

fn context() -> FileContext {
    FileContext::new(
        ProjectId::new("goldeneye").unwrap(),
        ProjectRelativePath::new("src/lib.rs").unwrap(),
    )
}

#[test]
fn default_edit_policy_requires_clean_proposed_syntax() {
    assert_eq!(
        EditOptions::default().parse_policy,
        ParsePolicy::RequireClean
    );
}

#[test]
fn replace_plans_one_named_node_and_reports_refresh_metadata() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(b"fn old() {}\nfn keep() {}".as_slice()),
            Generation::new(7),
        )
        .unwrap();
    let locator = all_named_locators(&snapshot, &context())
        .unwrap()
        .into_iter()
        .find(|locator| locator.anchor.node_kind == "function_item")
        .unwrap();

    let plan = plan_edit(
        &engine,
        &snapshot,
        &context(),
        &locator,
        &EditOperation::Replace("fn changed() {}".to_owned()),
        Generation::new(8),
        &EditOptions::default(),
    )
    .unwrap();

    assert_eq!(plan.source.as_ref(), b"fn changed() {}\nfn keep() {}");
    assert_eq!(plan.old_file_hash, snapshot.file_hash());
    assert_eq!(plan.new_file_hash, plan.snapshot.file_hash());
    assert_ne!(plan.old_file_hash, plan.new_file_hash);
    assert_eq!(plan.snapshot.generation(), Generation::new(8));
    assert!(!plan.snapshot.has_errors());
    assert_eq!(plan.diagnostics.after_total, 0);
    assert!(!plan.refreshed_locators.is_empty());
    assert!(plan.token_size.compact_syntax_bytes > 0);
    assert!(plan.diff.old_span.start < plan.diff.old_span.end);
    assert!(plan.diff.new_span.start < plan.diff.new_span.end);
}

#[test]
fn stale_locator_returns_typed_cause_and_fresh_compact_view() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(b"fn old() {}".as_slice()),
            Generation::new(7),
        )
        .unwrap();
    let mut locator = all_named_locators(&snapshot, &context())
        .unwrap()
        .into_iter()
        .find(|locator| locator.anchor.node_kind == "function_item")
        .unwrap();
    locator.scope.generation = Generation::new(6);

    let Err(error) = plan_edit(
        &engine,
        &snapshot,
        &context(),
        &locator,
        &EditOperation::Delete,
        Generation::new(8),
        &EditOptions::default(),
    ) else {
        panic!("stale locator unexpectedly planned an edit");
    };

    match error {
        EditError::StaleLocator { cause, fresh } => {
            assert_eq!(cause, LocatorError::GenerationMismatch);
            assert_eq!(fresh.scope.file_hash, snapshot.file_hash());
            assert_eq!(fresh.scope.generation, snapshot.generation());
            assert!(!fresh.nodes.is_empty());
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn create_content_validation_reports_size_and_rejects_parse_errors() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let source = Arc::<[u8]>::from(b"fn made() {}".as_slice());
    let valid = validate_create_content(
        &engine,
        LanguageId::new("rust").unwrap(),
        Arc::clone(&source),
        Generation::new(1),
        ParsePolicy::RequireClean,
    )
    .unwrap();

    assert_eq!(valid.source, source);
    assert_eq!(valid.content_hash, valid.snapshot.file_hash());
    assert_eq!(valid.token_size.source_bytes, source.len());
    assert_eq!(valid.token_size.changed_bytes, source.len());
    assert!(valid.token_size.approximate_context_tokens > 0);

    let invalid = validate_create_content(
        &engine,
        LanguageId::new("rust").unwrap(),
        Arc::<[u8]>::from(b"fn broken(".as_slice()),
        Generation::new(1),
        ParsePolicy::RequireClean,
    );
    assert!(matches!(
        invalid,
        Err(EditError::ParseRejected {
            policy: ParsePolicy::RequireClean,
            after_total,
            ..
        }) if after_total > 0
    ));
}
