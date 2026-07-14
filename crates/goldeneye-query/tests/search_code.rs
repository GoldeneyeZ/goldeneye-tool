mod common;

use common::Fixture;
use goldeneye_query::{QueryError, SearchCodeMode, SearchCodeRequest, SearchCodeResult};

#[test]
fn search_code_classifies_deduplicates_and_ranks_indexed_source_matches() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = SearchCodeRequest::new(fixture.project.clone(), "beta");
    request.context = 1;

    let SearchCodeResult::Matches(result) = engine.search_code(&request).expect("code search")
    else {
        panic!("expected match result");
    };
    assert_eq!(result.total_grep_matches, 3);
    assert_eq!(result.total_results, 3);
    assert_eq!(result.raw_match_count, 0);
    assert!(result.results.iter().all(|hit| !hit.match_lines.is_empty()));
    assert!(result.results.iter().all(|hit| hit.context.is_some()));
    assert!(result.directories.contains_key("src/"));
    assert_eq!(result.results[0].in_degree, 3);
}

#[test]
fn search_code_supports_ordered_literal_words_regex_filters_and_files_mode() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut ordered = SearchCodeRequest::new(fixture.project.clone(), "pub beta");
    ordered.file_pattern = Some("*.rs".to_owned());
    let SearchCodeResult::Matches(result) = engine.search_code(&ordered).expect("ordered literal")
    else {
        panic!("expected matches");
    };
    assert_eq!(result.total_grep_matches, 3);
    assert!(result.results.iter().any(|hit| hit.node == "beta"));

    let mut files = SearchCodeRequest::new(fixture.project.clone(), "Café");
    files.mode = SearchCodeMode::Files;
    let SearchCodeResult::Files(result) = engine.search_code(&files).expect("file mode") else {
        panic!("expected files");
    };
    assert_eq!(result.files, vec!["src/lib.rs"]);

    let mut invalid = SearchCodeRequest::new(fixture.project.clone(), "(");
    invalid.regex = true;
    assert!(matches!(
        engine.search_code(&invalid),
        Err(QueryError::InvalidPattern {
            field: "pattern",
            ..
        })
    ));

    let mut invalid_filter = SearchCodeRequest::new(fixture.project.clone(), "beta");
    invalid_filter.path_filter = Some("[".to_owned());
    assert!(matches!(
        engine.search_code(&invalid_filter),
        Err(QueryError::InvalidPattern {
            field: "path_filter",
            ..
        })
    ));
}

#[test]
fn full_search_code_returns_match_anchored_source_without_ascii_loss() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = SearchCodeRequest::new(fixture.project.clone(), "Café");
    request.mode = SearchCodeMode::Full;

    let SearchCodeResult::Matches(result) = engine.search_code(&request).expect("full search")
    else {
        panic!("expected matches");
    };
    let cafe = result
        .results
        .iter()
        .find(|hit| hit.node == "Café")
        .expect("Café result");
    assert!(
        cafe.source
            .as_deref()
            .is_some_and(|source| source.contains("Café"))
    );
}
