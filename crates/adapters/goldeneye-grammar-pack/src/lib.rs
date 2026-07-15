//! Verified grammar-pack lock, source, and materialized-cache integrity.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::string::FromUtf8Error;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

mod git_source;

use git_source::GitSourceSession;

const ASSET_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-assets-v1\0";
const NATIVE_SUPPORT_HASH_DOMAIN: &[u8] = b"goldeneye-native-support-assets-v1\0";
const LOCK_HASH_DOMAIN: &[u8] = b"goldeneye-grammar-lock-v1\0";
const BUFFER_SIZE: usize = 1024 * 1024;

pub const PACK_STATE_FILE: &str = "pack-state.json";

#[derive(Debug, Error)]
pub enum PackError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid grammar lock TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("grammar lock {path} is not UTF-8: {source}")]
    Utf8 {
        path: PathBuf,
        #[source]
        source: FromUtf8Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid grammar lock: {0}")]
    Invalid(String),
    #[error("grammar asset hash mismatch for {grammar}: expected {expected}, got {actual}")]
    HashMismatch {
        grammar: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarPackLock {
    schema_version: u32,
    upstream_repository: String,
    upstream_commit: String,
    declared_grammar_count: usize,
    declared_language_binding_count: usize,
    #[serde(default)]
    declared_native_support_count: usize,
    compatible_abi_min: u32,
    compatible_abi_max: u32,
    hash_algorithm: String,
    hash_domain: String,
    pub grammars: Vec<GrammarRecord>,
    #[serde(default)]
    pub native_support: Vec<NativeSupportRecord>,
    pub language_mappings: Vec<LanguageMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarRecord {
    pub name: String,
    pub repository: String,
    pub commit: Option<String>,
    pub missing_commit_reason: Option<String>,
    pub abi: u32,
    pub exported_symbol: String,
    pub assets: Vec<String>,
    pub source_hash: String,
    pub scanner_language: String,
    pub license_files: Vec<String>,
    pub verdict: String,
    #[serde(default)]
    pub provenance_notes: Vec<String>,
    pub orphan_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSupportRecord {
    pub name: String,
    pub repository: String,
    pub commit: Option<String>,
    pub missing_commit_reason: Option<String>,
    pub hash_domain: String,
    pub assets: Vec<String>,
    pub source_hash: String,
    pub license_files: Vec<String>,
    pub verdict: String,
    #[serde(default)]
    pub provenance_notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LanguageBindingStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageMapping {
    pub language_id: String,
    pub status: LanguageBindingStatus,
    pub grammar: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct VerifiedPack {
    pub grammar_count: usize,
    pub asset_count: usize,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarPackState {
    schema_version: u32,
    lock_hash: String,
    upstream_commit: String,
    grammar_count: usize,
    asset_count: usize,
}

mod materialized;
mod metadata;
mod path_safety;
mod source;
mod state;
mod streaming;
mod validation;

pub use materialized::{hash_grammar_assets, lock_file_hash, verify_materialized_pack};
use materialized::{lock_bytes_hash, stream_grammar_assets, stream_native_support_assets};
use path_safety::{
    ensure_safe_absolute_components, hex_digest, invalid, is_reparse_or_symlink, open_regular_file,
    open_rooted_directory, require_nonempty, validate_opened_regular_file,
};
use source::{
    SourceSession, ensure_safe_existing_directory, validate_asset_path, validate_component,
    validate_exported_symbol, validate_hash, validate_relative_path, validate_sorted_unique,
};
