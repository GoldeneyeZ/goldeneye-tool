use std::sync::Arc;

use goldeneye_domain::{FileContext, Generation, LanguageId, ProjectId, ProjectRelativePath};
use goldeneye_edit::{
    EditError, EditOperation, EditOptions, EditPlanRequest, ParsePolicy, plan_edit,
    validate_create_content,
};
use goldeneye_syntax::{
    CoreGrammarProvider, InspectRequest, SyntaxEngine, SyntaxSnapshot, all_named_locators,
    inspect_syntax,
};

fn context() -> FileContext {
    FileContext::new(
        ProjectId::new("goldeneye").unwrap(),
        ProjectRelativePath::new("src/lib.rs").unwrap(),
    )
}

fn request(
    snapshot: &SyntaxSnapshot,
    locator: &goldeneye_domain::NodeLocator,
    operation: EditOperation,
    parse_policy: ParsePolicy,
) -> EditPlanRequest {
    EditPlanRequest {
        language_id: snapshot.language_id().clone(),
        source: Arc::from(snapshot.source()),
        current_generation: snapshot.generation(),
        file_context: context(),
        locator: locator.clone(),
        operation,
        next_generation: Generation::new(snapshot.generation().value() + 1),
        options: EditOptions {
            parse_policy,
            ..EditOptions::default()
        },
    }
}

#[test]
fn proposed_parse_errors_are_rejected_or_returned_by_explicit_policy() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(b"fn old() {}".as_slice()),
            Generation::new(1),
        )
        .unwrap();
    let locator = all_named_locators(&snapshot, &context())
        .unwrap()
        .into_iter()
        .find(|locator| locator.anchor.node_kind == "function_item")
        .unwrap();

    let rejected = plan_edit(
        &engine,
        &request(
            &snapshot,
            &locator,
            EditOperation::Replace("fn broken(".to_owned()),
            ParsePolicy::RequireClean,
        ),
    );
    assert!(matches!(
        rejected,
        Err(EditError::ParseRejected {
            policy: ParsePolicy::RequireClean,
            before_total: 0,
            after_total,
            diagnostics,
            ..
        }) if after_total > 0 && !diagnostics.is_empty()
    ));

    let allowed = plan_edit(
        &engine,
        &request(
            &snapshot,
            &locator,
            EditOperation::Replace("fn broken(".to_owned()),
            ParsePolicy::AllowErrors,
        ),
    )
    .unwrap();
    assert!(allowed.diagnostics.after_total > 0);
    assert!(!allowed.diagnostics.after.is_empty());
}

#[test]
fn malformed_pre_edit_source_is_inspectable_and_can_be_improved_by_policy() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine
        .parse(
            LanguageId::new("rust").unwrap(),
            Arc::<[u8]>::from(b"fn broken( { }\nfn keep() {}".as_slice()),
            Generation::new(4),
        )
        .unwrap();
    assert!(snapshot.has_errors());
    assert!(snapshot.diagnostic_total() > 0);

    let inspection = inspect_syntax(&snapshot, &context(), &InspectRequest::default()).unwrap();
    assert!(!inspection.nodes.is_empty());
    let identifier = all_named_locators(&snapshot, &context())
        .unwrap()
        .into_iter()
        .find(|locator| locator.anchor.node_kind == "identifier")
        .expect("malformed fixture keeps an addressable identifier");

    let accepted = plan_edit(
        &engine,
        &request(
            &snapshot,
            &identifier,
            EditOperation::Replace("renamed".to_owned()),
            ParsePolicy::NoAdditionalDiagnostics,
        ),
    )
    .unwrap();
    assert!(accepted.diagnostics.before_total > 0);
    assert!(accepted.diagnostics.after_total <= accepted.diagnostics.before_total);

    let require_clean = plan_edit(
        &engine,
        &request(
            &snapshot,
            &identifier,
            EditOperation::Replace("renamed".to_owned()),
            ParsePolicy::RequireClean,
        ),
    );
    assert!(matches!(
        require_clean,
        Err(EditError::ParseRejected {
            policy: ParsePolicy::RequireClean,
            ..
        })
    ));
}

#[test]
fn create_validation_has_no_filesystem_side_effect_and_obeys_policy() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let valid = validate_create_content(
        &engine,
        LanguageId::new("rust").unwrap(),
        Arc::<[u8]>::from(b"fn created() {}".as_slice()),
        Generation::new(1),
        &context(),
        ParsePolicy::RequireClean,
    )
    .unwrap();
    assert_eq!(valid.diagnostics.after_total, 0);

    let rejected = validate_create_content(
        &engine,
        LanguageId::new("rust").unwrap(),
        Arc::<[u8]>::from(b"fn created(".as_slice()),
        Generation::new(1),
        &context(),
        ParsePolicy::NoAdditionalDiagnostics,
    );
    assert!(matches!(rejected, Err(EditError::ParseRejected { .. })));

    let allowed = validate_create_content(
        &engine,
        LanguageId::new("rust").unwrap(),
        Arc::<[u8]>::from(b"fn created(".as_slice()),
        Generation::new(1),
        &context(),
        ParsePolicy::AllowErrors,
    )
    .unwrap();
    assert!(allowed.diagnostics.after_total > 0);
}
