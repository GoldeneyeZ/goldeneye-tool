use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Classifies a source path without exposing adapter-owned language registries.
pub trait LanguageClassifier: Send + Sync {
    /// Returns the language selected for `path`, or `None` when it is unsupported.
    fn classify(&self, path: &Path) -> Option<LanguageId>;
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

/// Coherent source discovery policy used by application services.
///
/// Implementations classify individual paths and discover repository files with
/// the same language policy.
pub trait SourceDiscovery: RepositoryDiscovery + LanguageClassifier {}

impl<T> SourceDiscovery for T where T: RepositoryDiscovery + LanguageClassifier + ?Sized {}

impl<T> LanguageClassifier for Arc<T>
where
    T: LanguageClassifier + ?Sized,
{
    fn classify(&self, path: &Path) -> Option<LanguageId> {
        self.as_ref().classify(path)
    }
}

impl<T> RepositoryDiscovery for Arc<T>
where
    T: RepositoryDiscovery + ?Sized,
{
    fn discover(
        &self,
        root: &Path,
        options: &RepositoryDiscoveryOptions,
    ) -> Result<RepositoryDiscoveryReport, PortError> {
        self.as_ref().discover(root, options)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use goldeneye_domain::LanguageId;

    use super::{
        IndexMode, LanguageClassifier, RepositoryDiscovery, RepositoryDiscoveryOptions,
        RepositoryDiscoveryReport, SourceDiscovery,
    };

    struct StubSourceDiscovery;

    impl LanguageClassifier for StubSourceDiscovery {
        fn classify(&self, path: &Path) -> Option<LanguageId> {
            (path.extension().and_then(|extension| extension.to_str()) == Some("rs"))
                .then(|| LanguageId::new("rust").expect("language ID"))
        }
    }

    impl RepositoryDiscovery for StubSourceDiscovery {
        fn discover(
            &self,
            root: &Path,
            _options: &RepositoryDiscoveryOptions,
        ) -> Result<RepositoryDiscoveryReport, crate::PortError> {
            Ok(RepositoryDiscoveryReport {
                files: Vec::new(),
                warnings: vec![root.display().to_string()],
            })
        }
    }

    #[test]
    fn discovery_defaults_preserve_existing_policy() {
        let options = RepositoryDiscoveryOptions::default();
        assert_eq!(options.mode, IndexMode::Full);
        assert_eq!(options.max_file_bytes, 512 * 1024 * 1024);
        assert!(options.collect_ignored);
        assert!(options.global_ignore_path.is_none());
        assert!(options.extension_overrides.is_empty());
    }

    #[test]
    fn arc_forwards_combined_source_discovery_operations() {
        let source: Arc<dyn SourceDiscovery> = Arc::new(StubSourceDiscovery);

        let language = <Arc<dyn SourceDiscovery> as LanguageClassifier>::classify(
            &source,
            Path::new("src/lib.rs"),
        )
        .expect("classified language");
        let report = <Arc<dyn SourceDiscovery> as RepositoryDiscovery>::discover(
            &source,
            Path::new("repository"),
            &RepositoryDiscoveryOptions::default(),
        )
        .expect("discovery report");

        assert_eq!(language.as_str(), "rust");
        assert_eq!(report.warnings, ["repository"]);
    }
}
