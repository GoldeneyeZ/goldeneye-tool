use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use goldeneye_domain::LanguageId;

use crate::PortError;

/// Indexing depth shared by repository discovery and source extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
    Moderate,
    Fast,
}

/// Application-owned repository discovery policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiscoveryOptions {
    pub mode: IndexMode,
    pub max_file_bytes: u64,
    pub collect_ignored: bool,
    pub global_ignore_path: Option<PathBuf>,
    pub extension_overrides: HashMap<OsString, LanguageId>,
}

impl Default for RepositoryDiscoveryOptions {
    fn default() -> Self {
        Self {
            mode: IndexMode::Full,
            max_file_bytes: 512 * 1024 * 1024,
            collect_ignored: true,
            global_ignore_path: None,
            extension_overrides: HashMap::new(),
        }
    }
}

/// Source file selected by repository discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySourceFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub language: LanguageId,
}

/// Discovery data required by the indexing use case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiscoveryReport {
    pub files: Vec<RepositorySourceFile>,
    pub warnings: Vec<String>,
}

/// Discovers supported source files without exposing filesystem-walker details.
pub trait RepositoryDiscovery: Send + Sync {
    /// Discovers supported files below `root` in deterministic path order.
    ///
    /// # Errors
    ///
    /// Returns a type-erased adapter error when the repository cannot be scanned.
    fn discover(
        &self,
        root: &Path,
        options: &RepositoryDiscoveryOptions,
    ) -> Result<RepositoryDiscoveryReport, PortError>;
}

#[cfg(test)]
mod tests {
    use super::{IndexMode, RepositoryDiscoveryOptions};

    #[test]
    fn discovery_defaults_preserve_existing_policy() {
        let options = RepositoryDiscoveryOptions::default();
        assert_eq!(options.mode, IndexMode::Full);
        assert_eq!(options.max_file_bytes, 512 * 1024 * 1024);
        assert!(options.collect_ignored);
        assert!(options.global_ignore_path.is_none());
        assert!(options.extension_overrides.is_empty());
    }
}
