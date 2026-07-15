//! Application-owned interfaces for external mechanisms.

mod artifact;
mod crosslink;
mod discovery;
mod edit;
mod edit_syntax;
mod error;
mod git;
mod index;
mod index_syntax;
mod inspection;
mod project_administration;
mod query;
mod repository;

pub use artifact::ArtifactPersistence;
pub use crosslink::CrossLinkRepository;
pub use discovery::{
    IndexMode, LanguageClassifier, RepositoryDiscovery, RepositoryDiscoveryOptions,
    RepositoryDiscoveryReport, RepositorySourceFile, SourceDiscovery,
};
pub use edit::{
    EditIndexer, EditJournalRecord, EditOperationId, EditOperationKind, EditPhase,
    EditRefreshResult, EditRefreshStatus, EditRepository, NewEditJournalRecord,
};
pub use edit_syntax::{
    EditDiagnosticKind, EditInspectRequest, EditSyntax, EditSyntaxCreate, EditSyntaxCreateRequest,
    EditSyntaxDiagnostic, EditSyntaxError, EditSyntaxInspect, EditSyntaxInspectRequest,
    EditSyntaxInspection, EditSyntaxMutation, EditSyntaxNodeView, EditSyntaxPlan,
    EditSyntaxPlanRequest, ServiceSyntax, SyntaxInspector,
};
pub use error::PortError;
pub use git::{
    DetectChangesOptions, DetectedChanges, GitCoChange, GitContext, GitFailure, GitFileHistory,
    GitHistory, GitPortError, GitRepository,
};
pub use index::IndexRepository;
pub use index_syntax::{
    IndexDiagnosticKind, IndexExtractedCall, IndexExtractedFile, IndexExtractedImport,
    IndexExtractedRelation, IndexExtractionRequest, IndexFileSyntaxDiagnostics,
    IndexSyntaxDiagnostic, IndexSyntaxExtractor,
};
pub use inspection::{
    DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES, DEFAULT_PREVIEW_CHARS, InspectError, InspectRequest,
    MAX_INSPECT_DEPTH, MAX_INSPECT_KIND_FILTERS, MAX_INSPECT_NODES, MAX_PREVIEW_CHARS,
    SyntaxInspection, SyntaxNodeView,
};
pub use project_administration::ProjectAdministrationRepository;
pub use query::{
    ConnectionSettings, GraphCounts, NodeSignatureRecord, NodeVectorRecord, QueryRepository,
    STORED_VECTOR_DIM, SchemaInfo, SearchHit, StoredVector, TokenVectorRecord,
};
pub use repository::RepositoryFactory;
