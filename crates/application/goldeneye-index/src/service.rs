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

/// Files that share a scope can define the same qualified name: the module of
/// several files, a C header and its source file, Go files of one package split
/// by build tags. Such nodes have one ID, so the graph keeps a single node per
/// ID, the one with the longest span (a definition rather than a declaration),
/// and the edges of every defining file attach to it. A node whose qualified
/// name is already taken by a node of another kind is dropped: one symbol is
/// lost instead of the whole index.
fn merge_duplicate_nodes(nodes: &mut Vec<GraphNode>) {
    let span_bytes = |node: &GraphNode| {
        node.source_span
            .as_ref()
            .map_or(0, |span| span.bytes.end.saturating_sub(span.bytes.start))
    };
    let mut index_by_id = BTreeMap::new();
    let mut qualified_names = BTreeSet::new();
    let mut merged: Vec<GraphNode> = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        if let Some(&index) = index_by_id.get(&node.id) {
            // A shared module keeps the node of its first file, as before.
            if node.label.as_str() != "Module" && span_bytes(&node) > span_bytes(&merged[index]) {
                merged[index] = node;
            }
            continue;
        }
        if !qualified_names.insert(node.qualified_name.as_str().to_owned()) {
            continue;
        }
        index_by_id.insert(node.id.clone(), merged.len());
        merged.push(node);
    }
    *nodes = merged;
}

/// Keeps edges between known nodes, once per identity: files that share a
/// merged node each contribute the same structural edge to it.
fn retain_valid_edges(nodes: &[GraphNode], edges: &mut Vec<GraphEdge>) {
    let node_ids = nodes.iter().map(|node| &node.id).collect::<BTreeSet<_>>();
    let mut identities = BTreeSet::new();
    edges.retain(|edge| {
        node_ids.contains(&edge.source)
            && node_ids.contains(&edge.target)
            && identities.insert((
                edge.source.clone(),
                edge.target.clone(),
                edge.kind.clone(),
                edge.discriminator.clone(),
            ))
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
