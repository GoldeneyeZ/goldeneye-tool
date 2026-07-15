//! Application-owned interfaces for external mechanisms.

mod crosslink;
mod discovery;
mod edit;
mod edit_syntax;
mod error;
mod query;

pub use crosslink::CrossLinkRepository;
pub use discovery::{
    IndexMode, RepositoryDiscovery, RepositoryDiscoveryOptions, RepositoryDiscoveryReport,
    RepositorySourceFile,
};
pub use edit::{
    EditIndexer, EditJournalRecord, EditOperationId, EditOperationKind, EditPhase,
    EditRefreshResult, EditRefreshStatus, EditRepository, NewEditJournalRecord,
};
pub use edit_syntax::{
    EditDiagnosticKind, EditInspectRequest, EditSyntax, EditSyntaxCreate, EditSyntaxCreateRequest,
    EditSyntaxDiagnostic, EditSyntaxError, EditSyntaxInspection, EditSyntaxMutation,
    EditSyntaxNodeView, EditSyntaxPlan, EditSyntaxPlanRequest,
};
pub use error::PortError;
pub use query::{
    ConnectionSettings, GraphCounts, NodeSignatureRecord, NodeVectorRecord, QueryRepository,
    STORED_VECTOR_DIM, SchemaInfo, SearchHit, StoredVector, TokenVectorRecord,
};
