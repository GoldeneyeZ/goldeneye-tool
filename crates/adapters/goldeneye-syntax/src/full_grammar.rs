use goldeneye_domain::LanguageId;
use goldeneye_full_grammars::{LookupResult, available_language_ids, lookup};
use tree_sitter::Language;

#[cfg(feature = "core-grammars")]
use crate::grammar::CoreGrammarProvider;
use crate::grammar::{Grammar, GrammarProvider, GrammarSource, SyntaxError};

#[derive(Debug, Clone, Copy, Default)]
pub struct FullGrammarProvider;

impl GrammarProvider for FullGrammarProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        let compiled = match lookup(language_id.as_str()) {
            Some(LookupResult::Available(grammar)) => grammar,
            Some(LookupResult::Unavailable { .. }) | None => {
                return Err(SyntaxError::UnsupportedGrammar {
                    language_id: language_id.clone(),
                });
            }
        };
        let metadata = compiled.metadata();
        let language: Language = compiled.language_fn().into();
        let raw_abi = language.abi_version();
        let actual = u32::try_from(raw_abi).map_err(|_| SyntaxError::GrammarAbiOverflow {
            language_id: language_id.clone(),
            abi: raw_abi,
        })?;
        if actual != metadata.abi {
            return Err(SyntaxError::GrammarAbiMismatch {
                language_id: language_id.clone(),
                expected: metadata.abi,
                actual,
            });
        }

        Ok(Grammar {
            language_id: language_id.clone(),
            language,
            abi: actual,
            source: GrammarSource::FullPack {
                grammar: metadata.name.into(),
                source_hash: metadata.source_hash.into(),
            },
        })
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        available_language_ids()
            .map(|id| LanguageId::new(id).expect("full grammar IDs are non-empty"))
            .collect()
    }
}

/// Preserves the built-in grammar identity for core languages and falls back
/// to the materialized full pack for every other supported language.
#[derive(Debug, Clone, Copy, Default)]
#[cfg(feature = "core-grammars")]
pub struct CoreFirstGrammarProvider;

#[cfg(feature = "core-grammars")]
impl GrammarProvider for CoreFirstGrammarProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        match CoreGrammarProvider.grammar(language_id) {
            Ok(grammar) => Ok(grammar),
            Err(SyntaxError::UnsupportedGrammar { .. }) => FullGrammarProvider.grammar(language_id),
            Err(error) => Err(error),
        }
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        let mut supported = CoreGrammarProvider.supported_ids();
        for language_id in FullGrammarProvider.supported_ids() {
            if !supported.contains(&language_id) {
                supported.push(language_id);
            }
        }
        supported
    }
}
