use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::identity::SyntaxIdentityError;

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
