//! Opt-in, verified native Tree-sitter grammar registry.
//!
//! The default feature lane exposes no native registry and requires no grammar
//! cache. Enable `compiled` only after materializing the locked pack.

#![deny(unsafe_code)]

#[cfg(feature = "compiled")]
#[allow(unsafe_code)]
mod generated {
    include!("generated.rs");
}

#[cfg(feature = "compiled")]
use tree_sitter_language::LanguageFn;

/// Immutable provenance and compatibility metadata for one callable grammar.
#[cfg(feature = "compiled")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrammarMetadata {
    pub name: &'static str,
    pub exported_symbol: &'static str,
    pub abi: u32,
    pub scanner_language: &'static str,
    pub source_hash: &'static str,
}

/// A safe, copied language factory paired with its locked metadata.
#[cfg(feature = "compiled")]
#[derive(Clone, Copy)]
pub struct CompiledGrammar {
    language_fn: LanguageFn,
    metadata: GrammarMetadata,
}

#[cfg(feature = "compiled")]
impl CompiledGrammar {
    #[must_use]
    pub const fn language_fn(self) -> LanguageFn {
        self.language_fn
    }

    #[must_use]
    pub const fn metadata(self) -> GrammarMetadata {
        self.metadata
    }
}

/// Result of looking up one of the 160 declared language IDs.
#[cfg(feature = "compiled")]
#[derive(Clone, Copy)]
pub enum LookupResult {
    Available(CompiledGrammar),
    Unavailable { reason: &'static str },
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn declared_language_count() -> usize {
    generated::DECLARED_LANGUAGE_COUNT
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn available_language_count() -> usize {
    generated::AVAILABLE_LANGUAGE_COUNT
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn unique_grammar_count() -> usize {
    generated::UNIQUE_GRAMMAR_COUNT
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn compiled_source_count() -> usize {
    generated::COMPILED_SOURCE_COUNT
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn embedded_lock_hash() -> &'static str {
    generated::FULL_PACK_LOCK_SHA256
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn upstream_commit() -> &'static str {
    generated::FULL_PACK_UPSTREAM_COMMIT
}

#[cfg(feature = "compiled")]
#[must_use]
pub const fn orphan_source_count() -> usize {
    generated::ORPHAN_SOURCE_COUNT
}

/// Iterate over all declared language IDs in lexical order.
#[cfg(feature = "compiled")]
#[must_use]
pub fn declared_language_ids() -> impl ExactSizeIterator<Item = &'static str> + Clone {
    generated::LANGUAGES.iter().map(|language| language.id)
}

/// Iterate over all available language IDs in lexical order.
#[cfg(feature = "compiled")]
pub fn available_language_ids() -> impl Iterator<Item = &'static str> + Clone {
    generated::LANGUAGES.iter().filter_map(|language| {
        matches!(
            language.availability,
            generated::GeneratedAvailability::Available { .. }
        )
        .then_some(language.id)
    })
}

/// Iterate over all unique callable grammar metadata in lexical order.
#[cfg(feature = "compiled")]
#[must_use]
pub fn grammar_metadata() -> impl ExactSizeIterator<Item = GrammarMetadata> + Clone {
    generated::GRAMMARS.iter().map(metadata_from_generated)
}

/// Look up a declared language ID with a binary search over the static table.
#[cfg(feature = "compiled")]
#[must_use]
pub fn lookup(language_id: &str) -> Option<LookupResult> {
    let index = generated::LANGUAGES
        .binary_search_by(|language| language.id.cmp(language_id))
        .ok()?;
    match generated::LANGUAGES[index].availability {
        generated::GeneratedAvailability::Available { grammar_index } => {
            grammar(grammar_index).map(LookupResult::Available)
        }
        generated::GeneratedAvailability::Unavailable { reason } => {
            Some(LookupResult::Unavailable { reason })
        }
    }
}

#[cfg(feature = "compiled")]
fn grammar(index: usize) -> Option<CompiledGrammar> {
    let metadata = generated::GRAMMARS
        .get(index)
        .map(metadata_from_generated)?;
    let language_fn = generated::language_fn(index)?;
    Some(CompiledGrammar {
        language_fn,
        metadata,
    })
}

#[cfg(feature = "compiled")]
const fn metadata_from_generated(grammar: &generated::GeneratedGrammar) -> GrammarMetadata {
    GrammarMetadata {
        name: grammar.name,
        exported_symbol: grammar.exported_symbol,
        abi: grammar.abi,
        scanner_language: grammar.scanner_language,
        source_hash: grammar.source_hash,
    }
}
