use goldeneye_domain::{DomainError, GraphIdentityError, ProjectRelativePath, SyntaxIdentityError};
use goldeneye_syntax::SyntaxError;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ExtractionError {
    #[error("syntax parse failed for {path:?}: {source}")]
    Syntax {
        path: ProjectRelativePath,
        #[source]
        source: SyntaxError,
    },
    #[error("invalid graph identity: {0}")]
    GraphIdentity(#[from] GraphIdentityError),
    #[error("invalid domain identity: {0}")]
    Domain(#[from] DomainError),
    #[error("invalid syntax identity: {0}")]
    SyntaxIdentity(#[from] SyntaxIdentityError),
    #[error("Tree-sitter coordinate cannot fit u64: {0}")]
    CoordinateOverflow(&'static str),
}
