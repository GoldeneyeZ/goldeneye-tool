//! Fast, deterministic repository indexing for Goldeneye.

mod extract;
mod identity;
mod language_specs;
mod service;
mod types;

pub use identity::{canonical_project, canonical_root_string, project_id_for_root};
pub use service::IndexService;
pub use types::{
    CancellationToken, FileRefreshResult, FileRefreshStatus, FileSyntaxDiagnostics, IndexError,
    IndexOptions, IndexResult, IndexStatus,
};
