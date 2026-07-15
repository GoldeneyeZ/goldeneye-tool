use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::UNIX_EPOCH;

use goldeneye_domain::{
    ContentHash, FileId, FileRecord, Generation, GraphEdge, GraphNode, ProjectId, ProjectRecord,
    ProjectRelativePath,
};
use goldeneye_ports::{
    IndexExtractedFile as ExtractedFile, IndexExtractionRequest as Candidate, IndexRepository,
    IndexSyntaxExtractor, RepositoryDiscovery, RepositorySourceFile,
};

use crate::project_graph::{branch_node, project_contains_file, project_has_branch, project_node};
use crate::{
    FileRefreshResult, FileRefreshStatus, FileSyntaxDiagnostics, IndexError, IndexOptions,
    IndexResult, IndexStatus, canonical_project,
};

struct FileGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

struct ChangeSet {
    new_files: usize,
    changed_files: usize,
    deleted_files: usize,
    unchanged_files: usize,
    parse_inputs: Vec<Candidate>,
    discovered_paths: BTreeSet<ProjectRelativePath>,
}

type ProjectGraph = (Vec<FileRecord>, Vec<GraphNode>, Vec<GraphEdge>);

pub struct IndexService<R> {
    repository: R,
    extractor: Arc<dyn IndexSyntaxExtractor>,
    options: IndexOptions,
    discovery: Box<dyn RepositoryDiscovery>,
}

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    #[must_use]
    pub fn new(
        repository: R,
        extractor: impl IndexSyntaxExtractor + 'static,
        options: IndexOptions,
        discovery: impl RepositoryDiscovery + 'static,
    ) -> Self {
        Self {
            repository,
            extractor: Arc::new(extractor),
            options,
            discovery: Box::new(discovery),
        }
    }

    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    #[must_use]
    pub const fn repository_mut(&mut self) -> &mut R {
        &mut self.repository
    }

    #[must_use]
    pub fn into_repository(self) -> R {
        self.repository
    }
}

mod candidates;
mod graph;
mod indexing;
mod parsing;
mod refresh;

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    fn enforce_file_limit(&self, actual: usize) -> Result<(), IndexError> {
        if let Some(limit) = self.options.max_files
            && actual > limit
        {
            return Err(IndexError::FileLimitExceeded { limit, actual });
        }
        Ok(())
    }

    fn ensure_not_cancelled(&self) -> Result<(), IndexError> {
        if self.options.cancellation.is_cancelled() {
            Err(IndexError::Cancelled)
        } else {
            Ok(())
        }
    }
}

fn deduplicate_shared_modules(nodes: &mut Vec<GraphNode>) {
    let mut seen = BTreeSet::new();
    nodes.retain(|node| {
        node.label.as_str() != "Module"
            || seen.insert((node.id.clone(), node.qualified_name.as_str().to_owned()))
    });
}

fn relative_path(path: &Path) -> Result<ProjectRelativePath, IndexError> {
    let mut normalized = String::new();
    for component in path.components() {
        let value = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8RelativePath(PathBuf::from(path)))?;
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(value);
    }
    Ok(ProjectRelativePath::new(normalized)?)
}

fn classify_changes(
    candidates: &[Candidate],
    existing: &BTreeMap<ProjectRelativePath, FileRecord>,
) -> ChangeSet {
    let mut new_files = 0;
    let mut changed_files = 0;
    let mut unchanged_files = 0;
    let mut parse_inputs = Vec::new();
    let discovered_paths = candidates
        .iter()
        .map(|candidate| candidate.record.id.path.clone())
        .collect::<BTreeSet<_>>();
    for candidate in candidates {
        match existing.get(&candidate.record.id.path) {
            None => {
                new_files += 1;
                parse_inputs.push(candidate.clone());
            }
            Some(previous) if previous.content_hash != candidate.record.content_hash => {
                changed_files += 1;
                parse_inputs.push(candidate.clone());
            }
            Some(_) => unchanged_files += 1,
        }
    }
    let deleted_files = existing
        .keys()
        .filter(|path| !discovered_paths.contains(*path))
        .count();
    ChangeSet {
        new_files,
        changed_files,
        deleted_files,
        unchanged_files,
        parse_inputs,
        discovered_paths,
    }
}
