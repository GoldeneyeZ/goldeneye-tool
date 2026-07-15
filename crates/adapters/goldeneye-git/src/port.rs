use std::path::Path;

use goldeneye_ports::{
    DetectChangesOptions as PortDetectChangesOptions, DetectedChanges as PortDetectedChanges,
    GitCoChange as PortGitCoChange, GitContext, GitFailure as PortGitFailure,
    GitFileHistory as PortGitFileHistory, GitHistory as PortGitHistory, GitPortError,
    GitRepository, PortError,
};

use crate::{
    DetectChangesOptions, DetectedChanges, GitCoChange, GitError, GitFailure, GitFileHistory,
    GitHistory, GitLimits,
};

/// Shell-free Git command adapter with bounded default execution limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitCommandRepository;

impl GitRepository for GitCommandRepository {
    fn validate_reference(&self, reference: &str) -> Result<(), GitPortError> {
        crate::validate_reference(reference).map_err(map_error)
    }

    fn resolve_context(
        &self,
        root: &Path,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GitContext, GitPortError> {
        let cancelled = || cancellation();
        crate::resolve_context(root, &cancelled, &GitLimits::default()).map_err(map_error)
    }

    fn collect_history(
        &self,
        root: &Path,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<PortGitHistory, GitPortError> {
        let cancelled = || cancellation();
        crate::collect_history(root, &cancelled, &GitLimits::default())
            .map(map_history)
            .map_err(map_error)
    }

    fn detect_changes(
        &self,
        root: &Path,
        options: &PortDetectChangesOptions,
        cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<PortDetectedChanges, GitPortError> {
        let cancelled = || cancellation();
        let options = DetectChangesOptions {
            base_branch: options.base_branch.clone(),
            since: options.since.clone(),
        };
        crate::detect_changes(root, &options, &cancelled, &GitLimits::default())
            .map(map_detected_changes)
            .map_err(map_error)
    }
}

fn map_error(error: GitError) -> GitPortError {
    match error {
        GitError::Cancelled => GitPortError::Cancelled,
        GitError::InvalidReference => GitPortError::InvalidReference,
        error @ (GitError::TimedOut(_)
        | GitError::OutputLimit { .. }
        | GitError::Spawn(_)
        | GitError::Output(_)) => GitPortError::Adapter(PortError::new(error)),
    }
}

fn map_history(history: GitHistory) -> PortGitHistory {
    let GitHistory { files, couplings } = history;
    PortGitHistory {
        files: files.into_iter().map(map_file_history).collect(),
        couplings: couplings.into_iter().map(map_cochange).collect(),
    }
}

fn map_file_history(file: GitFileHistory) -> PortGitFileHistory {
    let GitFileHistory {
        path,
        change_count,
        last_modified,
    } = file;
    PortGitFileHistory {
        path,
        change_count,
        last_modified,
    }
}

fn map_cochange(coupling: GitCoChange) -> PortGitCoChange {
    let GitCoChange {
        file_a,
        file_b,
        co_changes,
        coupling_score,
        last_co_change,
    } = coupling;
    PortGitCoChange {
        file_a,
        file_b,
        co_changes,
        coupling_score,
        last_co_change,
    }
}

fn map_detected_changes(changes: DetectedChanges) -> PortDetectedChanges {
    let DetectedChanges { files, failure } = changes;
    PortDetectedChanges {
        files,
        failure: failure.map(map_failure),
    }
}

fn map_failure(failure: GitFailure) -> PortGitFailure {
    let GitFailure { status, reference } = failure;
    PortGitFailure { status, reference }
}

#[cfg(test)]
mod tests {
    use goldeneye_ports::{GitPortError, GitRepository};

    use super::GitCommandRepository;

    fn adapter_context_is_port_context(value: crate::GitContext) -> goldeneye_ports::GitContext {
        value
    }

    fn port_context_is_adapter_context(value: goldeneye_ports::GitContext) -> crate::GitContext {
        value
    }

    #[test]
    fn port_preserves_invalid_reference_cancellation_and_non_git_defaults() {
        let _ =
            adapter_context_is_port_context as fn(crate::GitContext) -> goldeneye_ports::GitContext;
        let _ =
            port_context_is_adapter_context as fn(goldeneye_ports::GitContext) -> crate::GitContext;
        let repository = GitCommandRepository;
        assert!(matches!(
            repository.validate_reference("--output=unsafe"),
            Err(GitPortError::InvalidReference)
        ));

        let root = tempfile::tempdir().expect("temp root");
        let cancelled = || true;
        assert!(matches!(
            repository.resolve_context(root.path(), &cancelled),
            Err(GitPortError::Cancelled)
        ));

        let active = || false;
        let context = repository
            .resolve_context(root.path(), &active)
            .expect("non-Git context");
        assert!(context.root_exists);
        assert!(!context.is_git);
        assert_eq!(
            context.branch_qualified_name("demo"),
            "demo.__branch__.working-tree"
        );
    }
}
