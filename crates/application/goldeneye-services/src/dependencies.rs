use std::sync::Arc;

use goldeneye_ports::{
    ArtifactPersistence, GitRepository, IndexSyntaxExtractor, LanguageClassifier,
    RepositoryFactory, ServiceSyntax, SourceDiscovery,
};

/// External mechanisms required by service use cases.
#[derive(Clone)]
pub struct ServiceDependencies {
    artifact: Arc<dyn ArtifactPersistence>,
    git: Arc<dyn GitRepository>,
    source: Arc<dyn SourceDiscovery>,
    repositories: Arc<dyn RepositoryFactory>,
    index_syntax: Arc<dyn IndexSyntaxExtractor>,
    edit_syntax: Arc<dyn ServiceSyntax>,
}

impl ServiceDependencies {
    #[must_use]
    pub fn new(
        artifact: Arc<dyn ArtifactPersistence>,
        git: Arc<dyn GitRepository>,
        source: Arc<dyn SourceDiscovery>,
        repositories: Arc<dyn RepositoryFactory>,
        index_syntax: Arc<dyn IndexSyntaxExtractor>,
        edit_syntax: Arc<dyn ServiceSyntax>,
    ) -> Self {
        Self {
            artifact,
            git,
            source,
            repositories,
            index_syntax,
            edit_syntax,
        }
    }

    pub(crate) fn artifact(&self) -> &dyn ArtifactPersistence {
        self.artifact.as_ref()
    }

    pub(crate) fn git(&self) -> &dyn GitRepository {
        self.git.as_ref()
    }

    pub(crate) fn discovery(&self) -> Arc<dyn SourceDiscovery> {
        Arc::clone(&self.source)
    }

    pub(crate) fn languages(&self) -> &dyn LanguageClassifier {
        self.source.as_ref()
    }

    pub(crate) fn repositories(&self) -> &dyn RepositoryFactory {
        self.repositories.as_ref()
    }

    pub(crate) fn index_syntax(&self) -> Arc<dyn IndexSyntaxExtractor> {
        Arc::clone(&self.index_syntax)
    }

    pub(crate) fn edit_syntax(&self) -> Arc<dyn ServiceSyntax> {
        Arc::clone(&self.edit_syntax)
    }
}
