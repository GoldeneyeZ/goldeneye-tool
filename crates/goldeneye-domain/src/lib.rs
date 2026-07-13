use thiserror::Error;

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
