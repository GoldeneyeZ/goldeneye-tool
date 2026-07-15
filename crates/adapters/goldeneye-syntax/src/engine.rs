mod edit_validation;
mod parse;

use std::sync::Arc;

use goldeneye_domain::{
    ContentHash, Generation, GrammarFingerprint, LanguageId, SourcePoint, SourceSpan,
};
use tree_sitter::Tree;

use crate::{Grammar, GrammarProvider, SyntaxError};
use edit_validation::validate_edit;
use parse::{
    grammar_fingerprint, parse_tree, range_to_span, snapshot_from_tree, validate_provider_language,
};

pub const MAX_DIAGNOSTIC_DETAILS: usize = 128;

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
        let language_id = validate_reparse_request(previous, language_id, generation)?;
        let grammar = self.reparse_grammar(previous, &language_id)?;
        let validated = validate_edit(previous.source(), &source, edit)?;
        let (tree, changed_ranges) =
            reparse_tree(previous, &grammar, &source, validated.input_edit)?;
        let snapshot = snapshot_from_tree(
            tree,
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

    fn reparse_grammar(
        &self,
        previous: &SyntaxSnapshot,
        language_id: &LanguageId,
    ) -> Result<Grammar, SyntaxError> {
        let grammar = self.provider.grammar(language_id)?;
        validate_provider_language(language_id, &grammar)?;
        let fingerprint = grammar_fingerprint(&grammar)?;
        if fingerprint != previous.grammar {
            return Err(SyntaxError::GrammarChanged {
                expected: Box::new(previous.grammar.clone()),
                actual: Box::new(fingerprint),
            });
        }
        Ok(grammar)
    }
}

fn validate_reparse_request(
    previous: &SyntaxSnapshot,
    language_id: LanguageId,
    generation: Generation,
) -> Result<LanguageId, SyntaxError> {
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
    Ok(language_id)
}

fn reparse_tree(
    previous: &SyntaxSnapshot,
    grammar: &Grammar,
    source: &[u8],
    input_edit: tree_sitter::InputEdit,
) -> Result<(Tree, Vec<SourceSpan>), SyntaxError> {
    let mut edited_old_tree = previous.tree.clone();
    edited_old_tree.edit(&input_edit);
    let new_tree = parse_tree(grammar, source, Some(&edited_old_tree))?;
    let changed_ranges = edited_old_tree
        .changed_ranges(&new_tree)
        .map(range_to_span)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((new_tree, changed_ranges))
}
