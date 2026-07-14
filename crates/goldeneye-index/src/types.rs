use std::io;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use goldeneye_discovery::{DiscoveryError, DiscoveryOptions, IndexMode};
use goldeneye_domain::{
    DomainError, Generation, GraphIdentityError, ProjectId, ProjectRecord, ProjectRelativePath,
    SyntaxIdentityError,
};
use goldeneye_store::{GraphCounts, StoreError};
use goldeneye_syntax::{SyntaxDiagnostic, SyntaxError};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub discovery: DiscoveryOptions,
    pub max_workers: NonZeroUsize,
    pub max_files: Option<usize>,
    pub cancellation: CancellationToken,
}

impl Default for IndexOptions {
    fn default() -> Self {
        let discovery = DiscoveryOptions {
            mode: IndexMode::Fast,
            ..DiscoveryOptions::default()
        };
        let workers = std::thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .min(8);
        Self {
            discovery,
            max_workers: NonZeroUsize::new(workers).expect("worker count is at least one"),
            max_files: None,
            cancellation: CancellationToken::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexStatus {
    Indexed,
    Unchanged,
    RejectedSyntax,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSyntaxDiagnostics {
    pub path: ProjectRelativePath,
    pub total: usize,
    pub truncated: bool,
    pub details: Vec<SyntaxDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexResult {
    pub status: IndexStatus,
    pub project: ProjectRecord,
    pub discovered_files: usize,
    pub new_files: usize,
    pub changed_files: usize,
    pub deleted_files: usize,
    pub unchanged_files: usize,
    pub parsed_files: usize,
    pub reused_files: usize,
    pub counts: GraphCounts,
    pub diagnostics: Vec<FileSyntaxDiagnostics>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRefreshStatus {
    Updated,
    Deleted,
    Unchanged,
    RejectedSyntax,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRefreshResult {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub status: FileRefreshStatus,
    pub generation: Generation,
    pub counts: GraphCounts,
    pub diagnostics: Vec<FileSyntaxDiagnostics>,
}

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(transparent)]
    CrossLink(#[from] goldeneye_crosslink::CrossLinkError),
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
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
    #[error("project root is not valid UTF-8: {0}")]
    NonUtf8Root(PathBuf),
    #[error("project-relative path is not valid UTF-8: {0}")]
    NonUtf8RelativePath(PathBuf),
    #[error("file metadata value cannot fit u64 for {path}: {field}")]
    MetadataOverflow { path: PathBuf, field: &'static str },
    #[error("repository contains {actual} supported files, above configured limit {limit}")]
    FileLimitExceeded { limit: usize, actual: usize },
    #[error("index operation was cancelled")]
    Cancelled,
    #[error("parallel index worker panicked")]
    WorkerPanicked,
    #[error("parallel index worker returned no result")]
    MissingWorkerResult,
    #[error("stored file graph has no File node: {0:?}")]
    MissingFileNode(ProjectRelativePath),
    #[error("Tree-sitter coordinate cannot fit u64: {0}")]
    CoordinateOverflow(&'static str),
    #[error("project generation overflow: {0:?}")]
    GenerationOverflow(ProjectId),
}
