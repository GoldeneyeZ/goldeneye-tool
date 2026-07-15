use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{
    PRETRAINED_DIM, PRETRAINED_TOKEN_COUNT, PRETRAINED_TOKENS_SHA256, PRETRAINED_VECTOR_SHA256,
};

const PRETRAINED_HEADER_LEN: usize = 8;

#[derive(Debug)]
pub struct PretrainedModel {
    token_indices: HashMap<Box<str>, usize>,
    vectors: Box<[i8]>,
}

impl PretrainedModel {
    /// Loads and checksum-verifies the pretrained token/vector assets.
    ///
    /// # Errors
    ///
    /// Returns an I/O, checksum, shape, token, or vector-length error for invalid assets.
    pub fn load(directory: impl AsRef<Path>) -> Result<Self, PretrainedModelError> {
        let directory = directory.as_ref();
        let vector_path = directory.join("code_vectors.bin");
        let token_path = directory.join("code_tokens.txt");
        let vector_bytes = read_verified(&vector_path, PRETRAINED_VECTOR_SHA256)?;
        let token_bytes = read_verified(&token_path, PRETRAINED_TOKENS_SHA256)?;
        let count = validate_vector_shape(&vector_bytes)?;
        let token_text = String::from_utf8(token_bytes)?;
        let token_indices = index_tokens(&token_text, count)?;
        let vectors = decode_vectors(&vector_bytes);
        Ok(Self {
            token_indices,
            vectors,
        })
    }

    /// Loads the pretrained model from Goldeneye's bundled asset directory.
    ///
    /// # Errors
    ///
    /// Returns the same verified asset errors as [`Self::load`].
    pub fn load_bundled() -> Result<Self, PretrainedModelError> {
        Self::load(Self::bundled_directory())
    }

    #[must_use]
    pub fn bundled_directory() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/nomic")
    }

    #[must_use]
    pub fn token_count(&self) -> usize {
        PRETRAINED_TOKEN_COUNT
    }

    #[must_use]
    pub fn lookup_token_count(&self) -> usize {
        self.token_indices.len()
    }

    #[must_use]
    pub fn vector(&self, token: &str) -> Option<&[i8]> {
        let index = *self.token_indices.get(token)?;
        let start = index * PRETRAINED_DIM;
        Some(&self.vectors[start..start + PRETRAINED_DIM])
    }
}

fn validate_vector_shape(vector_bytes: &[u8]) -> Result<usize, PretrainedModelError> {
    let count = read_header_word(vector_bytes, 0)?;
    let dimension = read_header_word(vector_bytes, 4)?;
    if count != PRETRAINED_TOKEN_COUNT || dimension != PRETRAINED_DIM {
        return Err(PretrainedModelError::InvalidShape { count, dimension });
    }
    let expected_len = PRETRAINED_HEADER_LEN + count * dimension;
    if vector_bytes.len() != expected_len {
        return Err(PretrainedModelError::InvalidVectorLength {
            expected: expected_len,
            actual: vector_bytes.len(),
        });
    }
    Ok(count)
}

fn index_tokens(
    token_text: &str,
    expected_count: usize,
) -> Result<HashMap<Box<str>, usize>, PretrainedModelError> {
    let mut token_indices = HashMap::with_capacity(expected_count);
    let mut token_count = 0;
    for (index, token) in token_text.lines().enumerate() {
        token_count += 1;
        // The audited vocabulary contains eleven deliberately empty rows;
        // upstream excludes them while preserving their vector row positions.
        if token.is_empty() {
            continue;
        }
        if token_indices
            .insert(Box::<str>::from(token), index)
            .is_some()
        {
            return Err(PretrainedModelError::DuplicateToken(token.to_owned()));
        }
    }
    if token_count != expected_count {
        return Err(PretrainedModelError::InvalidTokenCount {
            expected: expected_count,
            actual: token_count,
        });
    }
    Ok(token_indices)
}

fn decode_vectors(vector_bytes: &[u8]) -> Box<[i8]> {
    vector_bytes[PRETRAINED_HEADER_LEN..]
        .iter()
        .map(|byte| i8::from_ne_bytes([*byte]))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn read_verified(path: &Path, expected_checksum: &str) -> Result<Vec<u8>, PretrainedModelError> {
    let bytes = fs::read(path).map_err(|source| PretrainedModelError::Io {
        path: path.to_owned(),
        source,
    })?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected_checksum {
        return Err(PretrainedModelError::Checksum {
            path: path.to_owned(),
            expected: expected_checksum.to_owned(),
            actual,
        });
    }
    Ok(bytes)
}

fn read_header_word(bytes: &[u8], offset: usize) -> Result<usize, PretrainedModelError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(PretrainedModelError::TruncatedHeader)?;
    let raw: [u8; 4] = raw
        .try_into()
        .map_err(|_| PretrainedModelError::TruncatedHeader)?;
    usize::try_from(u32::from_le_bytes(raw)).map_err(|_| PretrainedModelError::TruncatedHeader)
}

#[derive(Debug, Error)]
pub enum PretrainedModelError {
    #[error("failed to read pretrained asset {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("pretrained asset checksum mismatch for {path}: expected {expected}, got {actual}")]
    Checksum {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("pretrained vector header is truncated")]
    TruncatedHeader,
    #[error("invalid pretrained vector shape {count}x{dimension}")]
    InvalidShape { count: usize, dimension: usize },
    #[error("invalid pretrained vector byte length: expected {expected}, got {actual}")]
    InvalidVectorLength { expected: usize, actual: usize },
    #[error("pretrained token file is not UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("invalid pretrained token count: expected {expected}, got {actual}")]
    InvalidTokenCount { expected: usize, actual: usize },
    #[error("duplicate pretrained token: {0}")]
    DuplicateToken(String),
}
