mod common;

use std::fs;

use common::Fixture;
use goldeneye_query::{
    CodeSnippetChunkRequest, CodeSnippetManifestRequest, CodeSnippetRequest, QueryError,
};

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

#[test]
fn manifest_and_chunk_return_stable_hashes_and_exact_source() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut manifest_request =
        CodeSnippetManifestRequest::new(fixture.project.clone(), "demo.src.lib");
    manifest_request.chunk_bytes = 256;

    let first_manifest = engine
        .get_code_snippet_manifest(&manifest_request)
        .expect("manifest");
    let second_manifest = engine
        .get_code_snippet_manifest(&manifest_request)
        .expect("repeat manifest");
    assert_eq!(first_manifest, second_manifest);
    assert_eq!(first_manifest.source_bytes, fixture.source.len());
    assert_eq!(first_manifest.chunk_count, 1);
    assert_eq!(first_manifest.source_sha256.len(), 64);
    assert!(
        first_manifest
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );

    let mut chunk_request =
        CodeSnippetChunkRequest::new(fixture.project.clone(), "demo.src.lib", 1);
    chunk_request.chunk_bytes = 256;
    chunk_request.expected_source_sha256 = Some(first_manifest.source_sha256.clone());
    let chunk = engine
        .get_code_snippet_chunk(&chunk_request)
        .expect("first chunk");
    assert_eq!(chunk.source, fixture.source);
    assert_eq!(chunk.chunk_start_byte, 0);
    assert_eq!(chunk.chunk_end_byte, fixture.source.len());
    assert_eq!(chunk.file_chunk_start_byte, first_manifest.start_byte);
    assert_eq!(chunk.file_chunk_end_byte, first_manifest.end_byte);
    assert!(chunk.eof);
    assert!(!chunk.truncated);
    assert_eq!(chunk.source_sha256, first_manifest.source_sha256);
    assert_eq!(chunk.indexed_file_hash, first_manifest.indexed_file_hash);
}

#[test]
fn manifest_and_chunk_reject_invalid_bounds_chunks_and_stale_hashes() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    for invalid in [255, 8_193] {
        let mut request = CodeSnippetManifestRequest::new(fixture.project.clone(), "Alpha");
        request.chunk_bytes = invalid;
        assert!(matches!(
            engine.get_code_snippet_manifest(&request),
            Err(QueryError::InvalidSnippetChunkBytes { actual, .. }) if actual == invalid
        ));
    }
    for valid in [256, 8_192] {
        let mut request = CodeSnippetManifestRequest::new(fixture.project.clone(), "Alpha");
        request.chunk_bytes = valid;
        assert!(engine.get_code_snippet_manifest(&request).is_ok());
    }
    for invalid_chunk in [0, 2] {
        let mut request =
            CodeSnippetChunkRequest::new(fixture.project.clone(), "Alpha", invalid_chunk);
        request.chunk_bytes = 256;
        assert!(matches!(
            engine.get_code_snippet_chunk(&request),
            Err(QueryError::InvalidSnippetChunk {
                actual,
                chunk_count: 1
            }) if actual == invalid_chunk
        ));
    }

    let mut stale = CodeSnippetChunkRequest::new(fixture.project.clone(), "Alpha", 1);
    stale.expected_source_sha256 = Some("0".repeat(64));
    assert!(matches!(
        engine.get_code_snippet_chunk(&stale),
        Err(QueryError::StaleSnippetSource { .. })
    ));

    let mut malformed = CodeSnippetChunkRequest::new(fixture.project.clone(), "Alpha", 1);
    malformed.expected_source_sha256 = Some("ABC".to_owned());
    assert!(matches!(
        engine.get_code_snippet_chunk(&malformed),
        Err(QueryError::InvalidSnippetArguments {
            field: "expected_source_sha256",
            ..
        })
    ));
}
