use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

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
