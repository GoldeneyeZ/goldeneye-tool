mod syntax;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

pub use syntax::{
    AncestorStep, ByteSpan, ContentHash, FileContext, Generation, GrammarFingerprint, LocatorScope,
    NodeAnchor, NodeLocator, ProjectRelativePath, SourcePoint, SourceSpan, SyntaxIdentityError,
};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("project ID must not be empty")]
    EmptyProjectId,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LanguageIdError {
    #[error("language ID cannot be empty")]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageId(String);

impl LanguageId {
    /// Creates a language identifier from a non-empty value.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageIdError::Empty`] when `value` is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, LanguageIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(LanguageIdError::Empty);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for LanguageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LanguageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    /// Creates a project identifier.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::EmptyProjectId`] when `value` is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DomainError::EmptyProjectId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ProjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{DomainError, ProjectId};

    #[test]
    fn project_id_rejects_empty_value() {
        assert_eq!(ProjectId::new(""), Err(DomainError::EmptyProjectId));
    }

    #[test]
    fn project_id_preserves_valid_value() {
        let id = ProjectId::new("sample").expect("valid project ID");
        assert_eq!(id.as_str(), "sample");
    }
}
