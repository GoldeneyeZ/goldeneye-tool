use std::{fmt, num::ParseIntError};

use thiserror::Error;
use xxhash_rust::xxh3::{xxh3_64, xxh3_64_with_seed};

pub const MINHASH_K: usize = 64;
pub const MINHASH_MIN_NODES: usize = 30;
pub const MINHASH_MIN_UNIQUE_TRIGRAMS: usize = 32;
pub const MINHASH_JACCARD_THRESHOLD: f64 = 0.95;
pub const MINHASH_MAX_EDGES: usize = 10;
pub const MINHASH_HEX_LEN: usize = MINHASH_K * 8;
pub const LSH_BANDS: usize = 32;
pub const LSH_ROWS: usize = 2;
pub const LSH_MAX_BUCKET_CANDIDATES: usize = 200;
pub const MAX_STRUCTURAL_TOKENS: usize = 4_096;

const TRIGRAM_BUFFER_LEN: usize = 160;
const TRIGRAM_WINDOW: usize = 2;
const MAX_STRUCTURAL_WEIGHT: usize = 3;
const UNIQUE_SET_SIZE: usize = 4_096;
const UNIQUE_SET_MASK: usize = UNIQUE_SET_SIZE - 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinHashSignature {
    values: [u32; MINHASH_K],
}

/// Compatibility name for the signature emitted by the upstream `simhash`
/// pipeline. The audited implementation uses weighted MinHash, not classic
/// bit-majority SimHash.
pub type SimHashSignature = MinHashSignature;

impl MinHashSignature {
    #[must_use]
    pub const fn from_values(values: [u32; MINHASH_K]) -> Self {
        Self { values }
    }

    #[must_use]
    pub const fn values(&self) -> &[u32; MINHASH_K] {
        &self.values
    }

    #[must_use]
    pub fn from_leaf_kinds<'a>(kinds: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let tokens = kinds
            .into_iter()
            .take(MAX_STRUCTURAL_TOKENS)
            .map(normalize_leaf_kind)
            .collect::<Vec<_>>();
        Self::from_normalized_tokens(&tokens)
    }

    #[must_use]
    pub fn from_normalized_tokens(tokens: &[&str]) -> Option<Self> {
        let tokens = &tokens[..tokens.len().min(MAX_STRUCTURAL_TOKENS)];
        if tokens.len() < MINHASH_MIN_NODES {
            return None;
        }

        let mut values = [u32::MAX; MINHASH_K];
        let mut unique = UniqueTrigramSet::default();
        for window in tokens.windows(TRIGRAM_WINDOW + 1) {
            let weight = window
                .iter()
                .filter(|token| !is_normalized_token(token))
                .count();
            if weight == 0 {
                continue;
            }
            let trigram = format!("{}|{}|{}", window[0], window[1], window[2]);
            if trigram.len() >= TRIGRAM_BUFFER_LEN {
                continue;
            }
            unique.insert(xxh3_64(trigram.as_bytes()));
            update_weighted_signature(&mut values, trigram.as_bytes(), weight);
        }

        (unique.count >= MINHASH_MIN_UNIQUE_TRIGRAMS).then_some(Self { values })
    }

    #[must_use]
    pub fn similarity(&self, other: &Self) -> f64 {
        let matching = self
            .values
            .iter()
            .zip(other.values)
            .filter(|(left, right)| **left == *right)
            .count();
        matching as f64 / MINHASH_K as f64
    }

    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut encoded = String::with_capacity(MINHASH_HEX_LEN);
        use fmt::Write as _;
        for value in self.values {
            write!(&mut encoded, "{value:08x}").expect("writing to String cannot fail");
        }
        encoded
    }

    pub fn from_hex(encoded: &str) -> Result<Self, MinHashDecodeError> {
        if encoded.len() != MINHASH_HEX_LEN {
            return Err(MinHashDecodeError::Length {
                expected: MINHASH_HEX_LEN,
                actual: encoded.len(),
            });
        }
        let mut values = [0_u32; MINHASH_K];
        for (index, value) in values.iter_mut().enumerate() {
            let start = index * 8;
            *value = u32::from_str_radix(&encoded[start..start + 8], 16).map_err(|source| {
                MinHashDecodeError::InvalidChunk { index, source }
            })?;
        }
        Ok(Self { values })
    }

    #[must_use]
    pub fn band_hashes(&self) -> [u16; LSH_BANDS] {
        std::array::from_fn(|band| {
            let base = band * LSH_ROWS;
            let mut bytes = [0_u8; LSH_ROWS * size_of::<u32>()];
            for row in 0..LSH_ROWS {
                let start = row * size_of::<u32>();
                bytes[start..start + size_of::<u32>()]
                    .copy_from_slice(&self.values[base + row].to_le_bytes());
            }
            (xxh3_64(&bytes) & u64::from(u16::MAX)) as u16
        })
    }
}

#[derive(Debug, Error)]
pub enum MinHashDecodeError {
    #[error("invalid MinHash hex length: expected {expected}, got {actual}")]
    Length { expected: usize, actual: usize },
    #[error("invalid MinHash hex chunk at index {index}: {source}")]
    InvalidChunk {
        index: usize,
        #[source]
        source: ParseIntError,
    },
}

#[must_use]
pub fn normalize_leaf_kind(kind: &str) -> &str {
    if matches!(
        kind,
        "identifier"
            | "field_identifier"
            | "property_identifier"
            | "type_identifier"
            | "shorthand_property_identifier"
            | "shorthand_field_identifier"
            | "variable_name"
            | "name"
    ) {
        "I"
    } else if matches!(
        kind,
        "string"
            | "string_literal"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "template_string"
            | "string_content"
            | "escape_sequence"
    ) {
        "S"
    } else if matches!(
        kind,
        "number"
            | "integer"
            | "float"
            | "integer_literal"
            | "float_literal"
            | "int_literal"
            | "number_literal"
    ) {
        "N"
    } else if matches!(
        kind,
        "predefined_type"
            | "primitive_type"
            | "builtin_type"
            | "type_annotation"
            | "simple_type"
    ) {
        "T"
    } else {
        kind
    }
}

fn is_normalized_token(token: &str) -> bool {
    matches!(token.as_bytes(), [b'I' | b'S' | b'N' | b'T'])
}

fn update_weighted_signature(
    signature: &mut [u32; MINHASH_K],
    trigram: &[u8],
    weight: usize,
) {
    for (index, minimum) in signature.iter_mut().enumerate() {
        for repetition in 0..weight {
            let seed = (index * MAX_STRUCTURAL_WEIGHT + repetition) as u64;
            let hash = xxh3_64_with_seed(trigram, seed) as u32;
            *minimum = (*minimum).min(hash);
        }
    }
}

struct UniqueTrigramSet {
    slots: [u64; UNIQUE_SET_SIZE],
    count: usize,
}

impl Default for UniqueTrigramSet {
    fn default() -> Self {
        Self {
            slots: [0; UNIQUE_SET_SIZE],
            count: 0,
        }
    }
}

impl UniqueTrigramSet {
    fn insert(&mut self, hash: u64) -> bool {
        let value = hash | 1;
        let slot = hash as usize & UNIQUE_SET_MASK;
        for probe in 0..UNIQUE_SET_SIZE {
            let index = (slot + probe) & UNIQUE_SET_MASK;
            if self.slots[index] == 0 {
                self.slots[index] = value;
                self.count += 1;
                return true;
            }
            if self.slots[index] == value {
                return false;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structural_tokens(prefix: &str) -> Vec<String> {
        (0..48).map(|index| format!("{prefix}_{index}")).collect()
    }

    #[test]
    fn leaf_normalization_matches_cross_language_categories() {
        assert_eq!(normalize_leaf_kind("identifier"), "I");
        assert_eq!(normalize_leaf_kind("type_identifier"), "I");
        assert_eq!(normalize_leaf_kind("raw_string_literal"), "S");
        assert_eq!(normalize_leaf_kind("float_literal"), "N");
        assert_eq!(normalize_leaf_kind("primitive_type"), "T");
        assert_eq!(normalize_leaf_kind("return"), "return");
    }

    #[test]
    fn weighted_minhash_is_deterministic_and_distinguishes_structure() {
        let first_tokens = structural_tokens("left");
        let second_tokens = structural_tokens("right");
        let first_refs = first_tokens.iter().map(String::as_str).collect::<Vec<_>>();
        let second_refs = second_tokens.iter().map(String::as_str).collect::<Vec<_>>();

        let first = MinHashSignature::from_normalized_tokens(&first_refs).expect("signature");
        let repeated = MinHashSignature::from_normalized_tokens(&first_refs).expect("signature");
        let second = MinHashSignature::from_normalized_tokens(&second_refs).expect("signature");

        assert_eq!(first, repeated);
        assert_eq!(first.similarity(&repeated), 1.0);
        assert!(first.similarity(&second) < 1.0);
    }

    #[test]
    fn short_or_structurally_empty_inputs_are_skipped() {
        assert!(MinHashSignature::from_normalized_tokens(&["return"; 29]).is_none());
        assert!(MinHashSignature::from_normalized_tokens(&["I"; 64]).is_none());
    }

    #[test]
    fn signature_hex_and_lsh_bands_are_stable() {
        let signature = MinHashSignature::from_values(std::array::from_fn(|index| {
            (index as u32).wrapping_mul(2_654_435_761)
        }));
        let encoded = signature.to_hex();

        assert_eq!(encoded.len(), MINHASH_HEX_LEN);
        assert_eq!(MinHashSignature::from_hex(&encoded).unwrap(), signature);
        assert_eq!(signature.band_hashes(), signature.band_hashes());
        assert!(MinHashSignature::from_hex(&encoded[..encoded.len() - 1]).is_err());
    }
}
