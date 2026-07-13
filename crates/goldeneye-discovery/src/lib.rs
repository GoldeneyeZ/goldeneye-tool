use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

mod ignore_rules;
mod language;
mod policy;
mod walker;

pub use ignore_rules::IgnoreRules;
pub use language::{LanguageRegistry, LanguageSpec};
pub use policy::{directory_policy, file_policy};
pub use walker::{MAX_IGNORED_DETAILS, discover};

pub const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
    Moderate,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageId(String);

impl LanguageId {
    /// Creates a language identifier from a non-empty value.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::InvalidLanguageId`] when `value` is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, DiscoveryError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DiscoveryError::InvalidLanguageId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOptions {
    pub mode: IndexMode,
    pub max_file_bytes: u64,
    pub collect_ignored: bool,
    pub global_ignore_path: Option<PathBuf>,
    pub extension_overrides: HashMap<OsString, LanguageId>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            mode: IndexMode::Full,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            collect_ignored: true,
            global_ignore_path: None,
            extension_overrides: HashMap::new(),
        }
    }
}

#[must_use]
pub fn parse_max_file_bytes(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_FILE_BYTES)
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("invalid repository root {path}: {source}")]
    InvalidRoot {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("repository root is not a directory: {path}")]
    NonDirectoryRoot { path: PathBuf },

    #[error("language ID cannot be empty")]
    InvalidLanguageId,

    #[error("invalid language data at line {line}: {detail}")]
    InvalidLanguageData { line: usize, detail: String },

    #[error("invalid ignore rule in {path}: {source}")]
    IgnoreRule {
        path: PathBuf,
        #[source]
        source: ignore::Error,
    },

    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub language: LanguageId,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    IgnoreRule,
    DirectoryPolicy,
    SuffixPolicy,
    FilenamePolicy,
    PatternPolicy,
    Oversized,
    UnsupportedLanguage,
    Symlink,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredPath {
    pub relative_path: PathBuf,
    pub reason: IgnoreReason,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub root: PathBuf,
    pub files: Vec<DiscoveredFile>,
    pub excluded_directories: Vec<PathBuf>,
    pub ignored: Vec<IgnoredPath>,
    pub ignored_total: usize,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream_discovery_limits() {
        let options = DiscoveryOptions::default();
        assert_eq!(options.mode, IndexMode::Full);
        assert_eq!(options.max_file_bytes, 512 * 1024 * 1024);
        assert!(options.collect_ignored);
    }

    #[test]
    fn max_file_bytes_accepts_positive_env_only() {
        assert_eq!(parse_max_file_bytes(Some("4096")), 4096);
        assert_eq!(parse_max_file_bytes(None), 512 * 1024 * 1024);
        assert_eq!(parse_max_file_bytes(Some("0")), 512 * 1024 * 1024);
        assert_eq!(parse_max_file_bytes(Some("-1")), 512 * 1024 * 1024);
        assert_eq!(parse_max_file_bytes(Some("invalid")), 512 * 1024 * 1024);
    }

    #[test]
    fn language_id_rejects_empty_values_and_preserves_valid_values() {
        assert!(matches!(
            LanguageId::new(""),
            Err(DiscoveryError::InvalidLanguageId)
        ));

        let language = LanguageId::new("rust").expect("rust is a valid language id");
        assert_eq!(language.as_str(), "rust");
    }
}
