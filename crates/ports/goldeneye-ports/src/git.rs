use std::error::Error;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::PortError;

/// Audited repository, worktree, branch, and revision metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct GitContext {
    pub input_path: String,
    pub is_git: bool,
    pub is_worktree: bool,
    pub is_detached: bool,
    pub root_exists: bool,
    pub worktree_root: String,
    pub git_dir: String,
    pub git_common_dir: String,
    pub canonical_root: String,
    pub branch: String,
    pub branch_slug: String,
    pub head_sha: String,
    pub base_sha: String,
}

impl GitContext {
    #[must_use]
    pub fn branch_qualified_name(&self, project: &str) -> String {
        let project = if project.is_empty() {
            "project"
        } else {
            project
        };
        let slug = if self.is_detached {
            "detached"
        } else if self.is_git && !self.branch_slug.is_empty() {
            &self.branch_slug
        } else {
            "working-tree"
        };
        format!("{project}.__branch__.{slug}")
    }
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

/// Stable application-facing Git failure categories.
#[derive(Debug)]
pub enum GitPortError {
    Cancelled,
    InvalidReference,
    Adapter(PortError),
}

impl fmt::Display for GitPortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Git operation was cancelled"),
            Self::InvalidReference => {
                formatter.write_str("base_branch contains invalid characters")
            }
            Self::Adapter(error) => error.fmt(formatter),
        }
    }
}

impl Error for GitPortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Adapter(error) => Some(error),
            Self::Cancelled | Self::InvalidReference => None,
        }
    }
}

/// Provides bounded Git context, history, and change discovery.
pub trait GitRepository: Send + Sync {
    /// Rejects references that cannot safely be passed as Git arguments.
    ///
    /// # Errors
    ///
    /// Returns [`GitPortError::InvalidReference`] for an unsafe reference.
    fn validate_reference(&self, reference: &str) -> Result<(), GitPortError>;

    /// Resolves repository and worktree metadata with cancellation polling.
    ///
    /// # Errors
    ///
    /// Returns a categorized cancellation or adapter execution error.
    fn resolve_context(
        &self,
        root: &Path,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GitContext, GitPortError>;

    /// Collects bounded file and co-change history with cancellation polling.
    ///
    /// # Errors
    ///
    /// Returns a categorized cancellation or adapter execution error.
    fn collect_history(
        &self,
        root: &Path,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GitHistory, GitPortError>;

    /// Detects committed and working-tree changes relative to the selected reference.
    ///
    /// # Errors
    ///
    /// Returns an invalid-reference, cancellation, or adapter execution error.
    fn detect_changes(
        &self,
        root: &Path,
        options: &DetectChangesOptions,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<DetectedChanges, GitPortError>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::GitContext;

    fn context() -> GitContext {
        GitContext {
            input_path: "repo".to_owned(),
            is_git: true,
            is_worktree: false,
            is_detached: false,
            root_exists: true,
            worktree_root: "repo".to_owned(),
            git_dir: ".git".to_owned(),
            git_common_dir: ".git".to_owned(),
            canonical_root: "repo".to_owned(),
            branch: "feature/test".to_owned(),
            branch_slug: "feature-test".to_owned(),
            head_sha: "head".to_owned(),
            base_sha: "base".to_owned(),
        }
    }

    #[test]
    fn git_context_serde_shape_and_branch_qualified_names_remain_stable() {
        let context = context();
        let encoded = serde_json::to_value(&context).expect("serialize context");
        let object = encoded.as_object().expect("context object");
        assert_eq!(object.len(), 13);
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "base_sha",
                "branch",
                "branch_slug",
                "canonical_root",
                "git_common_dir",
                "git_dir",
                "head_sha",
                "input_path",
                "is_detached",
                "is_git",
                "is_worktree",
                "root_exists",
                "worktree_root",
            ]
        );
        assert_eq!(
            serde_json::from_value::<GitContext>(encoded.clone()).expect("round trip"),
            context
        );

        let mut with_unknown = encoded.clone();
        with_unknown
            .as_object_mut()
            .expect("context object")
            .insert("unknown".to_owned(), json!(true));
        assert_eq!(
            serde_json::from_value::<GitContext>(with_unknown).expect("unknown field is ignored"),
            context
        );

        let mut missing = encoded;
        missing
            .as_object_mut()
            .expect("context object")
            .remove("branch");
        assert!(
            serde_json::from_value::<GitContext>(missing)
                .expect_err("missing field must fail")
                .to_string()
                .contains("missing field `branch`")
        );

        assert_eq!(
            context.branch_qualified_name("demo"),
            "demo.__branch__.feature-test"
        );
        assert_eq!(
            context.branch_qualified_name(""),
            "project.__branch__.feature-test"
        );
        let detached = GitContext {
            is_detached: true,
            ..context.clone()
        };
        assert_eq!(
            detached.branch_qualified_name("demo"),
            "demo.__branch__.detached"
        );
        let non_git = GitContext {
            is_git: false,
            ..context
        };
        assert_eq!(
            non_git.branch_qualified_name("demo"),
            "demo.__branch__.working-tree"
        );
    }
}
