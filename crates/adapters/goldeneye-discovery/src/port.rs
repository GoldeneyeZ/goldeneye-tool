use goldeneye_ports::{
    IndexMode as PortIndexMode, LanguageClassifier, PortError, RepositoryDiscovery,
    RepositoryDiscoveryOptions, RepositoryDiscoveryReport, RepositorySourceFile,
};

use crate::{DiscoveryOptions, IndexMode, LanguageId, LanguageRegistry, discover};

/// Filesystem-backed repository discovery adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileSystemDiscovery;

impl LanguageClassifier for FileSystemDiscovery {
    fn classify(&self, path: &std::path::Path) -> Option<LanguageId> {
        LanguageRegistry::upstream().classify(path).cloned()
    }
}

impl RepositoryDiscovery for FileSystemDiscovery {
    fn discover(
        &self,
        root: &std::path::Path,
        options: &RepositoryDiscoveryOptions,
    ) -> Result<RepositoryDiscoveryReport, PortError> {
        let options = DiscoveryOptions {
            mode: match options.mode {
                PortIndexMode::Full => IndexMode::Full,
                PortIndexMode::Moderate => IndexMode::Moderate,
                PortIndexMode::Fast => IndexMode::Fast,
            },
            max_file_bytes: options.max_file_bytes,
            collect_ignored: options.collect_ignored,
            global_ignore_path: options.global_ignore_path.clone(),
            extension_overrides: options.extension_overrides.clone(),
        };
        let report = discover(root, &options).map_err(PortError::new)?;
        Ok(RepositoryDiscoveryReport {
            files: report
                .files
                .into_iter()
                .map(|file| RepositorySourceFile {
                    absolute_path: file.absolute_path,
                    relative_path: file.relative_path,
                    language: file.language,
                })
                .collect(),
            warnings: report.warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use goldeneye_domain::LanguageId;
    use goldeneye_ports::LanguageClassifier;

    use super::FileSystemDiscovery;

    #[test]
    fn classifier_preserves_upstream_path_precedence() {
        let classifier = FileSystemDiscovery;

        assert_eq!(
            classifier.classify(Path::new("nested/Dockerfile")),
            Some(LanguageId::new("dockerfile").expect("language id"))
        );
        assert_eq!(
            classifier.classify(Path::new("views/page.blade.php")),
            Some(LanguageId::new("blade").expect("language id"))
        );
        assert_eq!(
            classifier.classify(Path::new("src/lib.rs")),
            Some(LanguageId::new("rust").expect("language id"))
        );
        assert_eq!(classifier.classify(Path::new("assets/data.unknown")), None);
    }
}
