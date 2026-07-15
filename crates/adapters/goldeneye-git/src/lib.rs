#![forbid(unsafe_code)]

//! Bounded, shell-free Git context, history, and change discovery.

use std::{io, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod changes;
mod context;
mod history;
mod port;
mod process;

pub use changes::{
    detect_changes, parse_hunks, parse_name_status, parse_range, validate_reference,
};
pub use context::resolve_context;
pub use goldeneye_ports::GitContext;
pub use history::{collect_history, is_trackable_file, parse_history_log};
pub use port::GitCommandRepository;

pub const MAX_HISTORY_COMMITS: usize = 10_000;
pub const MAX_FILES_PER_COMMIT: usize = 20;
pub const MAX_COUPLINGS: usize = 8_192;
pub const MAX_FILE_HISTORY: usize = 16_384;
pub const MIN_CO_CHANGES: u64 = 3;
pub const MIN_COUPLING_SCORE: f64 = 0.3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLimits {
    pub max_output_bytes: usize,
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for GitLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 32 * 1024 * 1024,
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(10),
        }
    }
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

impl<F> Cancellation for F
where
    F: Fn() -> bool + Send + Sync,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

#[derive(Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("Git operation was cancelled")]
    Cancelled,
    #[error("Git operation timed out after {0:?}")]
    TimedOut(Duration),
    #[error("Git output exceeded the {limit}-byte limit")]
    OutputLimit { limit: usize },
    #[error("cannot start Git: {0}")]
    Spawn(#[source] io::Error),
    #[error("cannot read Git output: {0}")]
    Output(#[source] io::Error),
    #[error("base_branch contains invalid characters")]
    InvalidReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFileHistory {
    pub path: String,
    pub change_count: u64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitCoChange {
    pub file_a: String,
    pub file_b: String,
    pub co_changes: u64,
    pub coupling_score: f64,
    pub last_co_change: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GitHistory {
    pub files: Vec<GitFileHistory>,
    pub couplings: Vec<GitCoChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedHunk {
    pub path: String,
    pub start_line: u64,
    pub end_line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectChangesOptions {
    pub base_branch: String,
    pub since: Option<String>,
}

impl Default for DetectChangesOptions {
    fn default() -> Self {
        Self {
            base_branch: "main".to_owned(),
            since: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFailure {
    pub status: i32,
    pub reference: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedChanges {
    pub files: Vec<String>,
    pub failure: Option<GitFailure>,
}

#[cfg(test)]
mod tests;
