use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use goldeneye_domain::{ContentHash, Generation, LanguageId, SourcePoint};
use goldeneye_syntax::{
    CoreGrammarProvider, DiagnosticKind, EditContentRegion, EditPointKind, Grammar,
    GrammarProvider, GrammarSource, MAX_DIAGNOSTIC_DETAILS, SyntaxEdit, SyntaxEngine, SyntaxError,
};

fn language(value: &str) -> LanguageId {
    LanguageId::new(value).unwrap()
}

fn source(bytes: &[u8]) -> Arc<[u8]> {
    Arc::from(bytes)
}

#[derive(Debug, Clone, Copy)]
struct FullPackFixtureProvider;

impl GrammarProvider for FullPackFixtureProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        if language_id.as_str() != "rust" {
            return Err(SyntaxError::UnsupportedGrammar {
                language_id: language_id.clone(),
            });
        }
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let abi = u32::try_from(language.abi_version()).expect("fixture ABI fits u32");
        Ok(Grammar {
            language_id: language_id.clone(),
            language,
            abi,
            source: GrammarSource::FullPack {
                grammar: "locked-rust-grammar".into(),
                source_hash: "abababababababababababababababababababababababababababababababab"
                    .into(),
            },
        })
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        vec![language("rust")]
    }
}

#[derive(Debug, Clone, Copy)]
struct MismatchedLanguageProvider;

impl GrammarProvider for MismatchedLanguageProvider {
    fn grammar(&self, _language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        Ok(inconsistent_grammar())
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        vec![language("rust")]
    }
}

#[derive(Debug, Default)]
struct MismatchAfterFirstLookupProvider {
    lookups: AtomicUsize,
}

impl GrammarProvider for MismatchAfterFirstLookupProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        if self.lookups.fetch_add(1, Ordering::Relaxed) == 0 {
            return CoreGrammarProvider.grammar(language_id);
        }
        Ok(inconsistent_grammar())
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        vec![language("rust")]
    }
}

fn inconsistent_grammar() -> Grammar {
    Grammar {
        language_id: language("python"),
        language: tree_sitter_rust::LANGUAGE.into(),
        abi: 0,
        source: GrammarSource::FullPack {
            grammar: String::new(),
            source_hash: String::new(),
        },
    }
}

fn point_for(bytes: &[u8], offset: usize) -> SourcePoint {
    let prefix = &bytes[..offset];
    let mut row = 0_usize;
    for byte in prefix {
        if *byte == b'\n' {
            row += 1;
        }
    }
    let column = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    SourcePoint::new(u64::try_from(row).unwrap(), u64::try_from(column).unwrap())
}

#[test]
fn snapshot_preserves_raw_bytes_hash_generation_language_and_fingerprint() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let raw = source(b"fn main() { let raw = \xff; }");
    let snapshot = engine
        .parse(language("rust"), Arc::clone(&raw), Generation::new(7))
        .unwrap();

    assert_eq!(snapshot.source(), raw.as_ref());
    assert_eq!(snapshot.generation(), Generation::new(7));
    assert_eq!(snapshot.language_id(), &language("rust"));
    assert_eq!(snapshot.root().kind(), "source_file");
    assert_eq!(snapshot.file_hash(), ContentHash::of(&raw));
    assert_eq!(snapshot.grammar().provider, "rust-crate");
    assert_eq!(snapshot.grammar().grammar, "rust");
    assert_eq!(snapshot.grammar().revision, "tree-sitter-rust@0.24.2");
    let abi = usize::try_from(snapshot.grammar().abi).unwrap();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi)
    );
}

#[test]
fn full_pack_snapshot_fingerprint_uses_locked_asset_hash_and_exact_abi() {
    let snapshot = SyntaxEngine::new(FullPackFixtureProvider)
        .parse(
            language("rust"),
            source(b"fn main() {}\n"),
            Generation::new(1),
        )
        .unwrap();

    assert_eq!(snapshot.grammar().provider, "full-pack");
    assert_eq!(snapshot.grammar().grammar, "locked-rust-grammar");
    assert_eq!(
        snapshot.grammar().revision,
        "abababababababababababababababababababababababababababababababab"
    );
    let expected_language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    assert_eq!(
        usize::try_from(snapshot.grammar().abi).unwrap(),
        expected_language.abi_version()
    );
}

#[test]
fn parse_rejects_provider_language_mismatch_before_fingerprinting_or_parsing() {
    let requested = language("rust");
    let result = SyntaxEngine::new(MismatchedLanguageProvider).parse(
        requested.clone(),
        source(b"fn main() {}\n"),
        Generation::new(1),
    );

    assert!(matches!(
        result,
        Err(SyntaxError::ProviderLanguageMismatch {
            requested: error_requested,
            returned,
        }) if error_requested == requested && returned == language("python")
    ));
}

#[test]
fn reparse_rejects_provider_language_mismatch_before_fingerprinting_or_parsing() {
    let engine = SyntaxEngine::new(MismatchAfterFirstLookupProvider::default());
    let requested = language("rust");
    let raw = source(b"fn main() {}\n");
    let old = engine
        .parse(requested.clone(), Arc::clone(&raw), Generation::new(1))
        .unwrap();
    let old_hash = old.file_hash();
    let no_op = SyntaxEdit::new(
        0,
        0,
        0,
        SourcePoint::new(0, 0),
        SourcePoint::new(0, 0),
        SourcePoint::new(0, 0),
    );

    let result = engine.reparse(
        &old,
        requested.clone(),
        Arc::clone(&raw),
        Generation::new(2),
        no_op,
    );
    assert!(matches!(
        result,
        Err(SyntaxError::ProviderLanguageMismatch {
            requested: error_requested,
            returned,
        }) if error_requested == requested && returned == language("python")
    ));
    assert_eq!(old.source(), raw.as_ref());
    assert_eq!(old.file_hash(), old_hash);
    assert_eq!(old.generation(), Generation::new(1));
    assert_eq!(old.root().end_byte(), raw.len());
}

#[test]
fn diagnostics_include_errors_missing_nodes_zero_width_and_raw_byte_columns() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let error_snapshot = engine
        .parse(language("python"), source(b"]\n"), Generation::new(1))
        .unwrap();
    assert!(
        error_snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::Error)
    );

    let incomplete = source("function café() {".as_bytes());
    let missing_snapshot = engine
        .parse(
            language("javascript"),
            Arc::clone(&incomplete),
            Generation::new(2),
        )
        .unwrap();
    let missing = missing_snapshot
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.kind == DiagnosticKind::Missing)
        .unwrap_or_else(|| {
            panic!(
                "incomplete JavaScript block must contain a missing token: {:?}",
                missing_snapshot.diagnostics()
            )
        });

    assert_eq!(missing.span.bytes.start, missing.span.bytes.end);
    assert_eq!(
        missing.span.start,
        point_for(
            &incomplete,
            usize::try_from(missing.span.bytes.start).unwrap()
        )
    );
    assert_eq!(missing.span.start.column_bytes, 18);
    assert_eq!(missing.span.end, missing.span.start);
}

#[test]
fn diagnostic_total_cap_and_preorder_match_the_tree_exactly() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let malformed = source("fn broken( {}\n".repeat(200).as_bytes());
    let snapshot = engine
        .parse(language("rust"), malformed, Generation::new(1))
        .unwrap();

    let mut expected = Vec::new();
    let mut cursor = snapshot.root().walk();
    loop {
        let node = cursor.node();
        if node.is_error() {
            expected.push((DiagnosticKind::Error, node));
        }
        if node.is_missing() {
            expected.push((DiagnosticKind::Missing, node));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break;
            }
        }
        if cursor.node() == snapshot.root() && !cursor.goto_next_sibling() {
            break;
        }
    }

    assert!(expected.len() > MAX_DIAGNOSTIC_DETAILS);
    assert_eq!(snapshot.diagnostic_total(), expected.len());
    assert_eq!(snapshot.diagnostics().len(), MAX_DIAGNOSTIC_DETAILS);
    assert!(snapshot.diagnostics_truncated());

    for (actual, (kind, node)) in snapshot.diagnostics().iter().zip(expected) {
        let range = node.range();
        assert_eq!(actual.kind, kind);
        assert_eq!(actual.node_kind, node.kind());
        assert_eq!(
            actual.span.bytes.start,
            u64::try_from(range.start_byte).unwrap()
        );
        assert_eq!(
            actual.span.bytes.end,
            u64::try_from(range.end_byte).unwrap()
        );
        assert_eq!(
            actual.span.start,
            SourcePoint::new(
                u64::try_from(range.start_point.row).unwrap(),
                u64::try_from(range.start_point.column).unwrap(),
            )
        );
        assert_eq!(
            actual.span.end,
            SourcePoint::new(
                u64::try_from(range.end_point.row).unwrap(),
                u64::try_from(range.end_point.column).unwrap(),
            )
        );
    }
}

#[test]
fn incremental_reparse_uses_byte_points_and_leaves_the_old_snapshot_immutable() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let old_source = source("fn café() {}\n".as_bytes());
    let old = engine
        .parse(
            language("rust"),
            Arc::clone(&old_source),
            Generation::new(4),
        )
        .unwrap();
    let old_hash = old.file_hash();
    let new_source = source("fn café_x<T>() {}\n".as_bytes());
    let edit = SyntaxEdit::new(
        8,
        8,
        13,
        SourcePoint::new(0, 8),
        SourcePoint::new(0, 8),
        SourcePoint::new(0, 13),
    );

    let reparsed = engine
        .reparse(
            &old,
            language("rust"),
            Arc::clone(&new_source),
            Generation::new(5),
            edit,
        )
        .unwrap();

    assert_eq!(reparsed.snapshot.source(), new_source.as_ref());
    assert_eq!(reparsed.snapshot.file_hash(), ContentHash::of(&new_source));
    assert_eq!(reparsed.snapshot.generation(), Generation::new(5));
    assert_eq!(reparsed.snapshot.root().kind(), "source_file");
    assert!(!reparsed.snapshot.has_errors());
    assert!(
        reparsed.changed_ranges.iter().any(|range| {
            range.bytes.start >= 8
                && range.bytes.start < range.bytes.end
                && range.bytes.end <= 13
                && range.bytes.end <= u64::try_from(new_source.len()).unwrap()
        }),
        "changed ranges: {:?}",
        reparsed.changed_ranges
    );

    assert_eq!(old.source(), old_source.as_ref());
    assert_eq!(old.file_hash(), old_hash);
    assert_eq!(old.generation(), Generation::new(4));
    assert_eq!(old.root().end_byte(), old_source.len());
}

#[test]
fn incremental_reparse_rejects_invalid_points_bounds_lengths_language_and_generation() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let old_source = source(b"fn main() {}\n");
    let old = engine
        .parse(language("rust"), old_source, Generation::new(4))
        .unwrap();
    let inserted = source(b"fn main_x() {}\n");

    let wrong_point = SyntaxEdit::new(
        7,
        7,
        9,
        SourcePoint::new(0, 6),
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 9),
    );
    assert!(matches!(
        engine.reparse(
            &old,
            language("rust"),
            Arc::clone(&inserted),
            Generation::new(5),
            wrong_point,
        ),
        Err(SyntaxError::EditPointMismatch {
            point: EditPointKind::Start,
            ..
        })
    ));

    let out_of_bounds = SyntaxEdit::new(
        7,
        u64::MAX,
        9,
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 9),
    );
    assert!(matches!(
        engine.reparse(
            &old,
            language("rust"),
            Arc::clone(&inserted),
            Generation::new(5),
            out_of_bounds,
        ),
        Err(SyntaxError::EditOffsetOutOfBounds { .. }
            | SyntaxError::OffsetConversionOverflow { .. })
    ));

    let wrong_length = SyntaxEdit::new(
        7,
        7,
        8,
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 8),
    );
    assert!(matches!(
        engine.reparse(
            &old,
            language("rust"),
            Arc::clone(&inserted),
            Generation::new(5),
            wrong_length,
        ),
        Err(SyntaxError::EditLengthMismatch { .. })
    ));

    let valid = SyntaxEdit::new(
        7,
        7,
        9,
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 9),
    );
    assert!(matches!(
        engine.reparse(
            &old,
            language("python"),
            Arc::clone(&inserted),
            Generation::new(5),
            valid,
        ),
        Err(SyntaxError::LanguageChanged { .. })
    ));
    assert!(matches!(
        engine.reparse(&old, language("rust"), inserted, Generation::new(4), valid,),
        Err(SyntaxError::NonIncreasingGeneration { .. })
    ));
}

#[test]
fn same_length_unrelated_rewrite_is_rejected_even_when_edit_geometry_is_valid() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let old = engine
        .parse(
            language("rust"),
            source(b"fn main() {}\n"),
            Generation::new(1),
        )
        .unwrap();
    let unrelated = source(b"fn nope() {}\n");
    let claimed_noop_at_end = SyntaxEdit::new(
        13,
        13,
        13,
        SourcePoint::new(0, 13),
        SourcePoint::new(0, 13),
        SourcePoint::new(0, 13),
    );

    assert!(matches!(
        engine.reparse(
            &old,
            language("rust"),
            unrelated,
            Generation::new(2),
            claimed_noop_at_end,
        ),
        Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Prefix
        })
    ));
}

#[test]
fn edit_rejects_unreported_changes_after_the_replacement() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let old = engine
        .parse(
            language("rust"),
            source(b"fn main() {}\n"),
            Generation::new(1),
        )
        .unwrap();
    let changed_replacement_and_suffix = source(b"fn nope() {x\n");
    let edit_claims_only_name_replacement = SyntaxEdit::new(
        3,
        7,
        7,
        SourcePoint::new(0, 3),
        SourcePoint::new(0, 7),
        SourcePoint::new(0, 7),
    );

    assert!(matches!(
        engine.reparse(
            &old,
            language("rust"),
            changed_replacement_and_suffix,
            Generation::new(2),
            edit_claims_only_name_replacement,
        ),
        Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Suffix
        })
    ));
}
