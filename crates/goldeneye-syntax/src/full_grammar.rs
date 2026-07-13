use goldeneye_domain::LanguageId;
use goldeneye_full_grammars::{LookupResult, available_language_ids, lookup};
use tree_sitter::Language;

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
