#![forbid(unsafe_code)]

//! Tree-sitter-backed graph fact extraction for repository indexing.

mod error;
mod extract;
mod language_specs;

use goldeneye_ports::{
    IndexExtractedFile, IndexExtractionRequest, IndexMode, IndexSyntaxExtractor, PortError,
};
use goldeneye_syntax::GrammarProvider;

/// Tree-sitter implementation of the repository indexing syntax boundary.
#[derive(Debug, Clone)]
pub struct TreeSitterIndexExtractor<P> {
    provider: P,
}

impl<P> TreeSitterIndexExtractor<P> {
    #[must_use]
    pub const fn new(provider: P) -> Self {
        Self { provider }
    }
}

impl<P> IndexSyntaxExtractor for TreeSitterIndexExtractor<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    fn supported_ids(&self) -> Vec<goldeneye_domain::LanguageId> {
        self.provider.supported_ids()
    }

    fn extract(
        &self,
        request: IndexExtractionRequest,
        mode: IndexMode,
    ) -> Result<IndexExtractedFile, PortError> {
        extract::extract(self.provider.clone(), request, mode).map_err(PortError::new)
    }
}
