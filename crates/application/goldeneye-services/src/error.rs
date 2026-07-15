use std::path::PathBuf;

use goldeneye_index::IndexError;
use goldeneye_ports::{GitPortError, PortError};
use goldeneye_query::QueryError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceErrorCode {
    Configuration,
    InvalidInput,
    Forbidden,
    NotFound,
    Cancelled,
    Storage,
    Index,
    Query,
    Conflict,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("cannot read current directory: {source}")]
    CurrentDirectory {
        #[source]
        source: std::io::Error,
    },
    #[error("cannot create database directory {path}: {source}")]
    DatabaseDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("repository path does not exist or cannot be resolved: {path}: {source}")]
    InvalidRepositoryPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("repository root is not a directory: {0}")]
    RepositoryNotDirectory(PathBuf),
    #[error("repo_path is outside the allowed root")]
    OutsideAllowedRoot,
    #[error("index operation was cancelled")]
    Cancelled,
    #[error(transparent)]
    Index(IndexError),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Git(#[from] GitPortError),
    #[error(transparent)]
    Artifact(#[from] PortError),
    #[error(transparent)]
    Repository(PortError),
    #[error(transparent)]
    CrossLink(#[from] goldeneye_crosslink::CrossLinkError),
    #[error("{message}")]
    Edit {
        code: ServiceErrorCode,
        message: String,
    },
}

impl ServiceError {
    #[must_use]
    pub const fn code(&self) -> ServiceErrorCode {
        match self {
            Self::CurrentDirectory { .. } | Self::DatabaseDirectory { .. } => {
                ServiceErrorCode::Configuration
            }
            Self::InvalidRepositoryPath { .. } | Self::RepositoryNotDirectory(_) => {
                ServiceErrorCode::InvalidInput
            }
            Self::OutsideAllowedRoot => ServiceErrorCode::Forbidden,
            Self::Cancelled
            | Self::Index(IndexError::Cancelled)
            | Self::Git(GitPortError::Cancelled) => ServiceErrorCode::Cancelled,
            Self::Artifact(_) | Self::Repository(_) => ServiceErrorCode::Storage,
            Self::Query(QueryError::ProjectNotFound(_)) => ServiceErrorCode::NotFound,
            Self::Query(_) => ServiceErrorCode::Query,
            Self::Git(GitPortError::InvalidReference) => ServiceErrorCode::InvalidInput,
            Self::Index(_) | Self::Git(_) | Self::CrossLink(_) => ServiceErrorCode::Index,
            Self::Edit { code, .. } => *code,
        }
    }
}
