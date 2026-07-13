use goldeneye_domain::{
    Generation, GrammarFingerprint, LanguageId, SourcePoint, SyntaxIdentityError,
};
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

    #[error("Tree-sitter rejected the grammar for language {language_id:?}")]
    IncompatibleGrammar { language_id: LanguageId },

    #[error("grammar provider returned language {returned:?} for requested language {requested:?}")]
    ProviderLanguageMismatch {
        requested: LanguageId,
        returned: LanguageId,
    },

    #[error("Tree-sitter cancelled parsing language {language_id:?}")]
    ParseCancelled { language_id: LanguageId },

    #[error("invalid grammar fingerprint for language {language_id:?}: {source}")]
    InvalidGrammarFingerprint {
        language_id: LanguageId,
        source: SyntaxIdentityError,
    },

    #[error("Tree-sitter coordinate {field} cannot be represented as u64")]
    TreeSitterCoordinateOverflow { field: &'static str },

    #[error("offset {value} for {field} cannot be represented as usize")]
    OffsetConversionOverflow { field: &'static str, value: u64 },

    #[error("source length for {field} cannot be represented as u64")]
    SourceLengthOverflow { field: &'static str },

    #[error("diagnostic count overflowed usize")]
    DiagnosticCountOverflow,

    #[error("Tree-sitter produced an invalid source span: {source}")]
    InvalidTreeSitterSpan { source: SyntaxIdentityError },

    #[error(
        "invalid edit bounds: start {start_byte}, old end {old_end_byte}, new end {new_end_byte}"
    )]
    InvalidEditBounds {
        start_byte: u64,
        old_end_byte: u64,
        new_end_byte: u64,
    },

    #[error("edit {field} offset {offset} exceeds source length {source_len}")]
    EditOffsetOutOfBounds {
        field: &'static str,
        offset: u64,
        source_len: u64,
    },

    #[error("edit length arithmetic overflowed")]
    EditLengthOverflow,

    #[error("edit predicts new length {expected}, but source length is {actual}")]
    EditLengthMismatch { expected: u64, actual: u64 },

    #[error("edit {point:?} point mismatch: expected {expected:?}, got {actual:?}")]
    EditPointMismatch {
        point: EditPointKind,
        expected: SourcePoint,
        actual: SourcePoint,
    },

    #[error("edit {region:?} bytes do not match between old and new source")]
    EditContentMismatch { region: EditContentRegion },

    #[error("cannot reparse language {actual:?} from snapshot language {expected:?}")]
    LanguageChanged {
        expected: LanguageId,
        actual: LanguageId,
    },

    #[error("cannot reparse with changed grammar metadata")]
    GrammarChanged {
        expected: Box<GrammarFingerprint>,
        actual: Box<GrammarFingerprint>,
    },

    #[error(
        "generation must increase when reparsing: previous {previous:?}, requested {requested:?}"
    )]
    NonIncreasingGeneration {
        previous: Generation,
        requested: Generation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPointKind {
    Start,
    OldEnd,
    NewEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditContentRegion {
    Prefix,
    Suffix,
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
