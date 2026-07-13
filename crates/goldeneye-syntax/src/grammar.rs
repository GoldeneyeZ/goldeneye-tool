use goldeneye_domain::LanguageId;
use thiserror::Error;
use tree_sitter::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grammar {
    pub language_id: LanguageId,
    pub language: Language,
    pub abi: u32,
    pub source: GrammarSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarSource {
    RustCrate {
        package: String,
        version: String,
    },
    FullPack {
        grammar: String,
        source_hash: String,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    #[error("no Tree-sitter grammar is available for language {language_id:?}")]
    UnsupportedGrammar { language_id: LanguageId },

    #[error("Tree-sitter grammar ABI {abi} for language {language_id:?} exceeds u32")]
    GrammarAbiOverflow { language_id: LanguageId, abi: usize },
}

pub trait GrammarProvider: Send + Sync {
    /// Returns the grammar and its reproducible provenance.
    ///
    /// # Errors
    ///
    /// Returns [`SyntaxError::UnsupportedGrammar`] when this provider does not
    /// expose `language_id`, or [`SyntaxError::GrammarAbiOverflow`] when the
    /// generated ABI cannot be represented by the public metadata type.
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError>;

    fn supported_ids(&self) -> Vec<LanguageId>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CoreGrammarProvider;

impl GrammarProvider for CoreGrammarProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        let (language, package, version): (Language, &str, &str) = match language_id.as_str() {
            "go" => (tree_sitter_go::LANGUAGE.into(), "tree-sitter-go", "0.25.0"),
            "javascript" => (
                tree_sitter_javascript::LANGUAGE.into(),
                "tree-sitter-javascript",
                "0.25.0",
            ),
            "python" => (
                tree_sitter_python::LANGUAGE.into(),
                "tree-sitter-python",
                "0.25.0",
            ),
            "rust" => (
                tree_sitter_rust::LANGUAGE.into(),
                "tree-sitter-rust",
                "0.24.2",
            ),
            "tsx" => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                "tree-sitter-typescript",
                "0.23.2",
            ),
            "typescript" => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "tree-sitter-typescript",
                "0.23.2",
            ),
            _ => {
                return Err(SyntaxError::UnsupportedGrammar {
                    language_id: language_id.clone(),
                });
            }
        };
        let raw_abi = language.abi_version();
        let abi = u32::try_from(raw_abi).map_err(|_| SyntaxError::GrammarAbiOverflow {
            language_id: language_id.clone(),
            abi: raw_abi,
        })?;

        Ok(Grammar {
            language_id: language_id.clone(),
            language,
            abi,
            source: GrammarSource::RustCrate {
                package: package.into(),
                version: version.into(),
            },
        })
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        ["go", "javascript", "python", "rust", "tsx", "typescript"]
            .into_iter()
            .map(|id| LanguageId::new(id).expect("core grammar IDs are non-empty"))
            .collect()
    }
}
