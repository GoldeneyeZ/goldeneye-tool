mod common;

use std::fs;

use common::Fixture;
use goldeneye_query::{CodeSnippetRequest, QueryError};

#[test]
fn snippet_resolves_exact_suffix_and_unique_short_names_to_exact_bytes() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    for query in ["demo.src.lib.Café.run", "src.lib.Café.run"] {
        let result = engine
            .get_code_snippet(&CodeSnippetRequest::new(fixture.project.clone(), query))
            .expect("method snippet");
        assert_eq!(result.source, "pub fn run() { beta(); }");
        assert_eq!((result.start_line, result.end_line), (4, 4));
        assert_eq!(result.symbol.qualified_name, "demo.src.lib.Café.run");
    }

    let short = engine
        .get_code_snippet(&CodeSnippetRequest::new(fixture.project.clone(), "Alpha"))
        .expect("short-name snippet");
    assert_eq!(short.source, "pub fn Alpha() { beta(); }");
    assert_eq!(short.start_byte, 0);
}

#[test]
fn unicode_span_is_byte_exact_and_missing_symbol_returns_ranked_suggestions() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let unicode = engine
        .get_code_snippet(&CodeSnippetRequest::new(
            fixture.project.clone(),
            "demo.src.lib.Café",
        ))
        .expect("Unicode snippet");
    assert_eq!(unicode.source, "pub struct Café;");
    assert_eq!(
        &fixture.source.as_bytes()[unicode.start_byte..unicode.end_byte],
        unicode.source.as_bytes()
    );

    match engine.get_code_snippet(&CodeSnippetRequest::new(fixture.project.clone(), "Alph")) {
        Err(QueryError::SymbolNotFound { suggestions, .. }) => assert_eq!(
            suggestions
                .iter()
                .map(|suggestion| suggestion.qualified_name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo.src.lib.Alpha"]
        ),
        other => panic!("expected suggestions, got {other:?}"),
    }
}

#[test]
fn snippet_rejects_stale_files_before_returning_source() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    fs::write(fixture.root.join("src/lib.rs"), "pub fn replaced() {}\n")
        .expect("mutate fixture source");

    assert!(matches!(
        engine.get_code_snippet(&CodeSnippetRequest::new(fixture.project.clone(), "Alpha",)),
        Err(QueryError::StaleFile { .. })
    ));
}

#[test]
fn snippet_limits_fail_closed_without_partial_source() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = CodeSnippetRequest::new(fixture.project.clone(), "Alpha");
    request.max_bytes = 5;

    assert!(matches!(
        engine.get_code_snippet(&request),
        Err(QueryError::SnippetTooLarge {
            actual_bytes: 26,
            maximum_bytes: 5,
            ..
        })
    ));
}
