use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{LanguageId, ProjectId};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SyntaxIdentityError {
    #[error("content hash must contain exactly 64 lowercase hexadecimal characters, got {actual}")]
    InvalidContentHashLength { actual: usize },
    #[error("content hash contains a non-lowercase-hexadecimal character at byte {index}")]
    InvalidContentHashCharacter { index: usize },
    #[error("byte span start {start} exceeds end {end}")]
    InvalidByteSpan { start: u64, end: u64 },
    #[error("source span start point must not follow its end point")]
    InvalidSourcePointOrder,
    #[error("project-relative path must not be empty")]
    EmptyProjectRelativePath,
    #[error("project-relative path must not be absolute")]
    AbsoluteProjectRelativePath,
    #[error("project-relative path must not be drive-prefixed")]
    DrivePrefixedProjectRelativePath,
    #[error("project-relative path must use forward slashes")]
    BackslashInProjectRelativePath,
    #[error("project-relative path must not contain empty segments")]
    EmptyProjectRelativePathSegment,
    #[error("project-relative path must not contain '.' segments")]
    CurrentProjectRelativePathSegment,
    #[error("project-relative path must not contain '..' segments")]
    ParentProjectRelativePathSegment,
    #[error("project-relative path must not contain NUL")]
    NulInProjectRelativePath,
    #[error("grammar provider must not be empty")]
    EmptyGrammarProvider,
    #[error("grammar name must not be empty")]
    EmptyGrammarName,
    #[error("grammar revision must not be empty")]
    EmptyGrammarRevision,
    #[error("grammar ABI must be greater than zero")]
    ZeroGrammarAbi,
    #[error("node kind must not be empty")]
    EmptyNodeKind,
    #[error("field name must not be empty when present")]
    EmptyFieldName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Generation(u64);

impl Generation {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Generation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::new(u64::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    #[must_use]
    pub fn of(bytes: impl AsRef<[u8]>) -> Self {
        Self(*blake3::hash(bytes.as_ref()).as_bytes())
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ContentHash {
    type Err = SyntaxIdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 {
            return Err(SyntaxIdentityError::InvalidContentHashLength {
                actual: value.len(),
            });
        }

        let mut bytes = [0_u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_lower_hex(pair[0])
                .ok_or(SyntaxIdentityError::InvalidContentHashCharacter { index: index * 2 })?;
            let low = decode_lower_hex(pair[1]).ok_or(
                SyntaxIdentityError::InvalidContentHashCharacter {
                    index: index * 2 + 1,
                },
            )?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

const fn decode_lower_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

impl Serialize for ContentHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}

impl ByteSpan {
    /// Creates an ordered half-open byte span.
    ///
    /// # Errors
    ///
    /// Returns [`SyntaxIdentityError::InvalidByteSpan`] when `start > end`.
    pub const fn new(start: u64, end: u64) -> Result<Self, SyntaxIdentityError> {
        if start > end {
            return Err(SyntaxIdentityError::InvalidByteSpan { start, end });
        }
        Ok(Self { start, end })
    }
}

impl<'de> Deserialize<'de> for ByteSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            start: u64,
            end: u64,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.start, raw.end).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SourcePoint {
    pub row: u64,
    pub column_bytes: u64,
}

impl SourcePoint {
    #[must_use]
    pub const fn new(row: u64, column_bytes: u64) -> Self {
        Self { row, column_bytes }
    }
}

impl<'de> Deserialize<'de> for SourcePoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            row: u64,
            column_bytes: u64,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(Self::new(raw.row, raw.column_bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct SourceSpan {
    pub bytes: ByteSpan,
    pub start: SourcePoint,
    pub end: SourcePoint,
}

impl SourceSpan {
    /// Creates a span with ordered byte and point bounds.
    ///
    /// # Errors
    ///
    /// Returns an error when either bound is reversed.
    pub const fn new(
        bytes: ByteSpan,
        start: SourcePoint,
        end: SourcePoint,
    ) -> Result<Self, SyntaxIdentityError> {
        if bytes.start > bytes.end {
            return Err(SyntaxIdentityError::InvalidByteSpan {
                start: bytes.start,
                end: bytes.end,
            });
        }
        if start.row > end.row || (start.row == end.row && start.column_bytes > end.column_bytes) {
            return Err(SyntaxIdentityError::InvalidSourcePointOrder);
        }
        Ok(Self { bytes, start, end })
    }
}

impl<'de> Deserialize<'de> for SourceSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            bytes: ByteSpan,
            start: SourcePoint,
            end: SourcePoint,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.bytes, raw.start, raw.end).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectRelativePath(String);

impl ProjectRelativePath {
    /// Creates a normalized, forward-slash-delimited project-relative path.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, absolute, drive-prefixed, backslash-delimited,
    /// NUL-containing, or non-normalized paths.
    pub fn new(value: impl Into<String>) -> Result<Self, SyntaxIdentityError> {
        let value = value.into();
        validate_project_relative_path(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_project_relative_path(value: &str) -> Result<(), SyntaxIdentityError> {
    if value.is_empty() {
        return Err(SyntaxIdentityError::EmptyProjectRelativePath);
    }
    if value.starts_with('/') {
        return Err(SyntaxIdentityError::AbsoluteProjectRelativePath);
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(SyntaxIdentityError::DrivePrefixedProjectRelativePath);
    }
    if value.contains('\\') {
        return Err(SyntaxIdentityError::BackslashInProjectRelativePath);
    }
    if value.contains('\0') {
        return Err(SyntaxIdentityError::NulInProjectRelativePath);
    }
    for segment in value.split('/') {
        match segment {
            "" => return Err(SyntaxIdentityError::EmptyProjectRelativePathSegment),
            "." => return Err(SyntaxIdentityError::CurrentProjectRelativePathSegment),
            ".." => return Err(SyntaxIdentityError::ParentProjectRelativePathSegment),
            _ => {}
        }
    }
    Ok(())
}

impl Serialize for ProjectRelativePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

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
