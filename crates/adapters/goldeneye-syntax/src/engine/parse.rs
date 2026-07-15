use std::{cell::RefCell, collections::HashMap, sync::Arc};

use goldeneye_domain::{
    ByteSpan, ContentHash, Generation, GrammarFingerprint, LanguageId, SourcePoint, SourceSpan,
};
use tree_sitter::{Parser, Point, Range, Tree};

use crate::{Grammar, GrammarSource, SyntaxError};

use super::{DiagnosticKind, MAX_DIAGNOSTIC_DETAILS, SyntaxDiagnostic, SyntaxSnapshot};

thread_local! {
    static PARSERS: RefCell<HashMap<String, Parser>> = RefCell::new(HashMap::new());
}

pub(super) fn validate_provider_language(
    requested: &LanguageId,
    grammar: &Grammar,
) -> Result<(), SyntaxError> {
    if grammar.language_id != *requested {
        return Err(SyntaxError::ProviderLanguageMismatch {
            requested: requested.clone(),
            returned: grammar.language_id.clone(),
        });
    }
    Ok(())
}

pub(super) fn grammar_fingerprint(grammar: &Grammar) -> Result<GrammarFingerprint, SyntaxError> {
    let (provider, grammar_name, revision) = match &grammar.source {
        GrammarSource::RustCrate { package, version } => (
            "rust-crate",
            grammar.language_id.as_str(),
            format!("{package}@{version}"),
        ),
        GrammarSource::FullPack {
            grammar,
            source_hash,
        } => ("full-pack", grammar.as_str(), source_hash.clone()),
    };
    GrammarFingerprint::new(provider, grammar_name, revision, grammar.abi).map_err(|source| {
        SyntaxError::InvalidGrammarFingerprint {
            language_id: grammar.language_id.clone(),
            source,
        }
    })
}

pub(super) fn parse_tree(
    grammar: &Grammar,
    source: &[u8],
    old_tree: Option<&Tree>,
) -> Result<Tree, SyntaxError> {
    PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        let parser = parsers
            .entry(grammar.language_id.as_str().to_owned())
            .or_insert_with(Parser::new);
        parser
            .set_language(&grammar.language)
            .map_err(|_| SyntaxError::IncompatibleGrammar {
                language_id: grammar.language_id.clone(),
            })?;
        parser
            .parse(source, old_tree)
            .ok_or_else(|| SyntaxError::ParseCancelled {
                language_id: grammar.language_id.clone(),
            })
    })
}

pub(super) fn snapshot_from_tree(
    tree: Tree,
    source: Arc<[u8]>,
    generation: Generation,
    language_id: LanguageId,
    grammar: GrammarFingerprint,
) -> Result<SyntaxSnapshot, SyntaxError> {
    let (diagnostics, diagnostic_total) = collect_diagnostics(&tree)?;
    let file_hash = ContentHash::of(&source);
    Ok(SyntaxSnapshot {
        tree,
        source,
        file_hash,
        generation,
        language_id,
        grammar,
        diagnostics,
        diagnostic_total,
    })
}

fn collect_diagnostics(tree: &Tree) -> Result<(Vec<SyntaxDiagnostic>, usize), SyntaxError> {
    let mut diagnostics = Vec::with_capacity(MAX_DIAGNOSTIC_DETAILS);
    let mut total = 0_usize;
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        if node.is_error() {
            record_diagnostic(&mut diagnostics, &mut total, DiagnosticKind::Error, node)?;
        }
        if node.is_missing() {
            record_diagnostic(&mut diagnostics, &mut total, DiagnosticKind::Missing, node)?;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok((diagnostics, total));
            }
        }
    }
}

fn record_diagnostic(
    diagnostics: &mut Vec<SyntaxDiagnostic>,
    total: &mut usize,
    kind: DiagnosticKind,
    node: tree_sitter::Node<'_>,
) -> Result<(), SyntaxError> {
    *total = total
        .checked_add(1)
        .ok_or(SyntaxError::DiagnosticCountOverflow)?;
    if diagnostics.len() < MAX_DIAGNOSTIC_DETAILS {
        diagnostics.push(SyntaxDiagnostic {
            kind,
            node_kind: node.kind().to_owned(),
            span: range_to_span(node.range())?,
        });
    }
    Ok(())
}

pub(super) fn range_to_span(range: Range) -> Result<SourceSpan, SyntaxError> {
    let bytes = ByteSpan::new(
        usize_to_u64("range.start_byte", range.start_byte)?,
        usize_to_u64("range.end_byte", range.end_byte)?,
    )
    .map_err(|source| SyntaxError::InvalidTreeSitterSpan { source })?;
    let start = tree_point_to_source("range.start_point", range.start_point)?;
    let end = tree_point_to_source("range.end_point", range.end_point)?;
    SourceSpan::new(bytes, start, end)
        .map_err(|source| SyntaxError::InvalidTreeSitterSpan { source })
}

fn tree_point_to_source(field: &'static str, point: Point) -> Result<SourcePoint, SyntaxError> {
    Ok(SourcePoint::new(
        usize_to_u64(field, point.row)?,
        usize_to_u64(field, point.column)?,
    ))
}

fn usize_to_u64(field: &'static str, value: usize) -> Result<u64, SyntaxError> {
    u64::try_from(value).map_err(|_| SyntaxError::TreeSitterCoordinateOverflow { field })
}
