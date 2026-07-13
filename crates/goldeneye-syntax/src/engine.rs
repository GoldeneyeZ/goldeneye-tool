use std::{cell::RefCell, collections::HashMap, sync::Arc};

use goldeneye_domain::{
    ByteSpan, ContentHash, Generation, GrammarFingerprint, LanguageId, SourcePoint, SourceSpan,
};
use tree_sitter::{InputEdit, Parser, Point, Range, Tree};

use crate::{
    EditContentRegion, EditPointKind, Grammar, GrammarProvider, GrammarSource, SyntaxError,
};

pub const MAX_DIAGNOSTIC_DETAILS: usize = 128;

thread_local! {
    static PARSERS: RefCell<HashMap<String, Parser>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Error,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub kind: DiagnosticKind,
    pub node_kind: String,
    pub span: SourceSpan,
}

pub struct SyntaxSnapshot {
    tree: Tree,
    source: Arc<[u8]>,
    file_hash: ContentHash,
    generation: Generation,
    language_id: LanguageId,
    grammar: GrammarFingerprint,
    diagnostics: Vec<SyntaxDiagnostic>,
    diagnostic_total: usize,
}

impl SyntaxSnapshot {
    #[must_use]
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    #[must_use]
    pub fn root(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    #[must_use]
    pub const fn file_hash(&self) -> ContentHash {
        self.file_hash
    }

    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    #[must_use]
    pub const fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    #[must_use]
    pub const fn grammar(&self) -> &GrammarFingerprint {
        &self.grammar
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.tree.root_node().has_error()
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub const fn diagnostic_total(&self) -> usize {
        self.diagnostic_total
    }

    #[must_use]
    pub fn diagnostics_truncated(&self) -> bool {
        self.diagnostic_total > self.diagnostics.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxEdit {
    start_byte: u64,
    old_end_byte: u64,
    new_end_byte: u64,
    start_position: SourcePoint,
    old_end_position: SourcePoint,
    new_end_position: SourcePoint,
}

impl SyntaxEdit {
    #[must_use]
    pub const fn new(
        start_byte: u64,
        old_end_byte: u64,
        new_end_byte: u64,
        start_position: SourcePoint,
        old_end_position: SourcePoint,
        new_end_position: SourcePoint,
    ) -> Self {
        Self {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position,
            old_end_position,
            new_end_position,
        }
    }
}

pub struct ReparseResult {
    pub snapshot: SyntaxSnapshot,
    pub changed_ranges: Vec<SourceSpan>,
}

pub struct SyntaxEngine<P> {
    provider: P,
}

impl<P> SyntaxEngine<P>
where
    P: GrammarProvider,
{
    #[must_use]
    pub const fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Parses raw source bytes into an immutable syntax snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed syntax error when no compatible grammar is available,
    /// parsing is cancelled, or Tree-sitter coordinates cannot be represented.
    pub fn parse(
        &self,
        language_id: LanguageId,
        source: Arc<[u8]>,
        generation: Generation,
    ) -> Result<SyntaxSnapshot, SyntaxError> {
        let grammar = self.provider.grammar(&language_id)?;
        validate_provider_language(&language_id, &grammar)?;
        let fingerprint = grammar_fingerprint(&grammar)?;
        let tree = parse_tree(&grammar, &source, None)?;
        snapshot_from_tree(tree, source, generation, language_id, fingerprint)
    }

    /// Incrementally reparses a snapshot without mutating it.
    ///
    /// # Errors
    ///
    /// Returns a typed error when language, grammar, generation, edit geometry,
    /// source continuity, or byte points are inconsistent.
    pub fn reparse(
        &self,
        previous: &SyntaxSnapshot,
        language_id: LanguageId,
        source: Arc<[u8]>,
        generation: Generation,
        edit: SyntaxEdit,
    ) -> Result<ReparseResult, SyntaxError> {
        if language_id != previous.language_id {
            return Err(SyntaxError::LanguageChanged {
                expected: previous.language_id.clone(),
                actual: language_id,
            });
        }
        if generation <= previous.generation {
            return Err(SyntaxError::NonIncreasingGeneration {
                previous: previous.generation,
                requested: generation,
            });
        }

        let grammar = self.provider.grammar(&language_id)?;
        validate_provider_language(&language_id, &grammar)?;
        let fingerprint = grammar_fingerprint(&grammar)?;
        if fingerprint != previous.grammar {
            return Err(SyntaxError::GrammarChanged {
                expected: Box::new(previous.grammar.clone()),
                actual: Box::new(fingerprint),
            });
        }

        let validated = validate_edit(previous.source(), &source, edit)?;
        let mut edited_old_tree = previous.tree.clone();
        edited_old_tree.edit(&validated.input_edit);
        let new_tree = parse_tree(&grammar, &source, Some(&edited_old_tree))?;
        let changed_ranges = edited_old_tree
            .changed_ranges(&new_tree)
            .map(range_to_span)
            .collect::<Result<Vec<_>, _>>()?;
        let snapshot = snapshot_from_tree(
            new_tree,
            source,
            generation,
            language_id,
            previous.grammar.clone(),
        )?;

        Ok(ReparseResult {
            snapshot,
            changed_ranges,
        })
    }
}

fn validate_provider_language(
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

fn grammar_fingerprint(grammar: &Grammar) -> Result<GrammarFingerprint, SyntaxError> {
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

fn parse_tree(
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

fn snapshot_from_tree(
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

fn range_to_span(range: Range) -> Result<SourceSpan, SyntaxError> {
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

struct ValidatedEdit {
    input_edit: InputEdit,
}

fn validate_edit(
    old_source: &[u8],
    new_source: &[u8],
    edit: SyntaxEdit,
) -> Result<ValidatedEdit, SyntaxError> {
    if edit.start_byte > edit.old_end_byte || edit.start_byte > edit.new_end_byte {
        return Err(SyntaxError::InvalidEditBounds {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
        });
    }

    let old_len = usize_to_source_len("old source", old_source.len())?;
    let new_len = usize_to_source_len("new source", new_source.len())?;
    ensure_bound("start byte", edit.start_byte, old_len)?;
    ensure_bound("old end byte", edit.old_end_byte, old_len)?;
    ensure_bound("new end byte", edit.new_end_byte, new_len)?;

    let start_byte = u64_to_usize("start byte", edit.start_byte)?;
    let old_end_byte = u64_to_usize("old end byte", edit.old_end_byte)?;
    let new_end_byte = u64_to_usize("new end byte", edit.new_end_byte)?;
    let removed = edit.old_end_byte - edit.start_byte;
    let inserted = edit.new_end_byte - edit.start_byte;
    let expected_new_len = old_len
        .checked_sub(removed)
        .and_then(|length| length.checked_add(inserted))
        .ok_or(SyntaxError::EditLengthOverflow)?;
    if expected_new_len != new_len {
        return Err(SyntaxError::EditLengthMismatch {
            expected: expected_new_len,
            actual: new_len,
        });
    }

    if old_source[..start_byte] != new_source[..start_byte] {
        return Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Prefix,
        });
    }
    if old_source[old_end_byte..] != new_source[new_end_byte..] {
        return Err(SyntaxError::EditContentMismatch {
            region: EditContentRegion::Suffix,
        });
    }
    validate_point(
        EditPointKind::Start,
        edit.start_position,
        source_point_at(old_source, start_byte)?,
    )?;
    validate_point(
        EditPointKind::OldEnd,
        edit.old_end_position,
        source_point_at(old_source, old_end_byte)?,
    )?;
    validate_point(
        EditPointKind::NewEnd,
        edit.new_end_position,
        source_point_at(new_source, new_end_byte)?,
    )?;

    Ok(ValidatedEdit {
        input_edit: InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position: source_point_to_tree("start position", edit.start_position)?,
            old_end_position: source_point_to_tree("old end position", edit.old_end_position)?,
            new_end_position: source_point_to_tree("new end position", edit.new_end_position)?,
        },
    })
}

fn usize_to_source_len(field: &'static str, value: usize) -> Result<u64, SyntaxError> {
    u64::try_from(value).map_err(|_| SyntaxError::SourceLengthOverflow { field })
}

fn ensure_bound(field: &'static str, offset: u64, source_len: u64) -> Result<(), SyntaxError> {
    if offset > source_len {
        return Err(SyntaxError::EditOffsetOutOfBounds {
            field,
            offset,
            source_len,
        });
    }
    Ok(())
}

fn u64_to_usize(field: &'static str, value: u64) -> Result<usize, SyntaxError> {
    usize::try_from(value).map_err(|_| SyntaxError::OffsetConversionOverflow { field, value })
}

fn source_point_at(source: &[u8], offset: usize) -> Result<SourcePoint, SyntaxError> {
    let prefix = &source[..offset];
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
    Ok(SourcePoint::new(
        usize_to_u64("source point row", row)?,
        usize_to_u64("source point column", column)?,
    ))
}

fn validate_point(
    point: EditPointKind,
    actual: SourcePoint,
    expected: SourcePoint,
) -> Result<(), SyntaxError> {
    if actual != expected {
        return Err(SyntaxError::EditPointMismatch {
            point,
            expected,
            actual,
        });
    }
    Ok(())
}

fn source_point_to_tree(field: &'static str, point: SourcePoint) -> Result<Point, SyntaxError> {
    Ok(Point::new(
        u64_to_usize(field, point.row)?,
        u64_to_usize(field, point.column_bytes)?,
    ))
}
