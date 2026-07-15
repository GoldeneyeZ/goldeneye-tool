use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{LanguageId, ProjectId};

use super::identity::{ContentHash, Generation, SyntaxIdentityError};
use super::source::{ProjectRelativePath, SourceSpan};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct GrammarFingerprint {
    pub provider: String,
    pub grammar: String,
    pub revision: String,
    pub abi: u32,
}

impl GrammarFingerprint {
    /// Creates reproducible grammar metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when a textual identity is empty or `abi` is zero.
    pub fn new(
        provider: impl Into<String>,
        grammar: impl Into<String>,
        revision: impl Into<String>,
        abi: u32,
    ) -> Result<Self, SyntaxIdentityError> {
        let provider = provider.into();
        let grammar = grammar.into();
        let revision = revision.into();
        if provider.is_empty() {
            return Err(SyntaxIdentityError::EmptyGrammarProvider);
        }
        if grammar.is_empty() {
            return Err(SyntaxIdentityError::EmptyGrammarName);
        }
        if revision.is_empty() {
            return Err(SyntaxIdentityError::EmptyGrammarRevision);
        }
        if abi == 0 {
            return Err(SyntaxIdentityError::ZeroGrammarAbi);
        }
        Ok(Self {
            provider,
            grammar,
            revision,
            abi,
        })
    }
}

impl<'de> Deserialize<'de> for GrammarFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            provider: String,
            grammar: String,
            revision: String,
            abi: u32,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.provider, raw.grammar, raw.revision, raw.abi).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FileContext {
    pub project_id: ProjectId,
    pub relative_path: ProjectRelativePath,
}

impl FileContext {
    #[must_use]
    pub const fn new(project_id: ProjectId, relative_path: ProjectRelativePath) -> Self {
        Self {
            project_id,
            relative_path,
        }
    }
}

impl<'de> Deserialize<'de> for FileContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            project_id: ProjectId,
            relative_path: ProjectRelativePath,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(Self::new(raw.project_id, raw.relative_path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LocatorScope {
    pub file: FileContext,
    pub language_id: LanguageId,
    pub grammar: GrammarFingerprint,
    pub file_hash: ContentHash,
    pub generation: Generation,
}

impl LocatorScope {
    #[must_use]
    pub const fn new(
        file: FileContext,
        language_id: LanguageId,
        grammar: GrammarFingerprint,
        file_hash: ContentHash,
        generation: Generation,
    ) -> Self {
        Self {
            file,
            language_id,
            grammar,
            file_hash,
            generation,
        }
    }
}

impl<'de> Deserialize<'de> for LocatorScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            file: FileContext,
            language_id: LanguageId,
            grammar: GrammarFingerprint,
            file_hash: ContentHash,
            generation: Generation,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(Self::new(
            raw.file,
            raw.language_id,
            raw.grammar,
            raw.file_hash,
            raw.generation,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct AncestorStep {
    pub node_kind: String,
    pub named_child_index: u32,
    pub field_name: Option<String>,
}

impl AncestorStep {
    /// Creates one stable ancestor component.
    ///
    /// # Errors
    ///
    /// Returns an error when the node kind or a present field name is empty.
    pub fn new(
        node_kind: impl Into<String>,
        named_child_index: u32,
        field_name: Option<String>,
    ) -> Result<Self, SyntaxIdentityError> {
        let node_kind = node_kind.into();
        if node_kind.is_empty() {
            return Err(SyntaxIdentityError::EmptyNodeKind);
        }
        if field_name.as_deref() == Some("") {
            return Err(SyntaxIdentityError::EmptyFieldName);
        }
        Ok(Self {
            node_kind,
            named_child_index,
            field_name,
        })
    }
}

impl<'de> Deserialize<'de> for AncestorStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            node_kind: String,
            named_child_index: u32,
            field_name: Option<String>,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.node_kind, raw.named_child_index, raw.field_name).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct NodeAnchor {
    pub ancestor_path: Vec<AncestorStep>,
    pub node_kind: String,
    pub source_span: SourceSpan,
    pub content_hash: ContentHash,
}

impl NodeAnchor {
    /// Creates a guarded node anchor.
    ///
    /// # Errors
    ///
    /// Returns an error when `node_kind` is empty.
    pub fn new(
        ancestor_path: Vec<AncestorStep>,
        node_kind: impl Into<String>,
        source_span: SourceSpan,
        content_hash: ContentHash,
    ) -> Result<Self, SyntaxIdentityError> {
        let node_kind = node_kind.into();
        if node_kind.is_empty() {
            return Err(SyntaxIdentityError::EmptyNodeKind);
        }
        Ok(Self {
            ancestor_path,
            node_kind,
            source_span,
            content_hash,
        })
    }
}

impl<'de> Deserialize<'de> for NodeAnchor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            ancestor_path: Vec<AncestorStep>,
            node_kind: String,
            source_span: SourceSpan,
            content_hash: ContentHash,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(
            raw.ancestor_path,
            raw.node_kind,
            raw.source_span,
            raw.content_hash,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct NodeLocator {
    pub scope: LocatorScope,
    pub anchor: NodeAnchor,
}

impl NodeLocator {
    #[must_use]
    pub const fn new(scope: LocatorScope, anchor: NodeAnchor) -> Self {
        Self { scope, anchor }
    }
}

impl<'de> Deserialize<'de> for NodeLocator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            scope: LocatorScope,
            anchor: NodeAnchor,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(Self::new(raw.scope, raw.anchor))
    }
}
