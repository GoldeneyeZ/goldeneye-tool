use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::UNIX_EPOCH;

use goldeneye_discovery::{DiscoveredFile, discover};
use goldeneye_domain::{
    ContentHash, FileId, FileRecord, Generation, GraphEdge, GraphNode, ProjectId, ProjectRecord,
    ProjectRelativePath,
};
use goldeneye_store::Store;
use goldeneye_syntax::GrammarProvider;

use crate::extract::{
    Candidate, ExtractedFile, branch_node, extract, project_contains_file, project_has_branch,
    project_node,
};
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

pub struct IndexService<P> {
    store: Store,
    provider: P,
    options: IndexOptions,
}

impl<P> IndexService<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    #[must_use]
    pub const fn new(store: Store, provider: P, options: IndexOptions) -> Self {
        Self {
            store,
            provider,
            options,
        }
    }

    #[must_use]
    pub const fn store(&self) -> &Store {
        &self.store
    }

    #[must_use]
    pub const fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    #[must_use]
    pub fn into_store(self) -> Store {
        self.store
    }

    /// Discovers and indexes a repository, reusing unchanged persisted file graphs.
    ///
    /// # Errors
    ///
    /// Returns a typed discovery, I/O, syntax, identity, cancellation, bound, or store error.
    /// No graph mutation occurs until all required parses and graph validation succeed.
    pub fn index_repository(&mut self, root: impl AsRef<Path>) -> Result<IndexResult, IndexError> {
        self.ensure_not_cancelled()?;
        let report = discover(root.as_ref(), &self.options.discovery)?;
        self.enforce_file_limit(report.files.len())?;
        self.ensure_not_cancelled()?;

        let mut project = canonical_project(root.as_ref())?;
        let stored_project = self.store.get_project(&project.id)?;
        if let Some(stored) = &stored_project {
            project.generation = stored.generation;
        }
        let candidates = self.read_candidates(&report.files, &project.id)?;
        let existing_files = if stored_project.is_some() {
            self.store.list_files(&project.id)?
        } else {
            Vec::new()
        };
        let existing = existing_files
            .iter()
            .cloned()
            .map(|file| (file.id.path.clone(), file))
            .collect::<BTreeMap<_, _>>();

        let ChangeSet {
            new_files,
            changed_files,
            deleted_files,
            unchanged_files,
            parse_inputs,
            discovered_paths,
        } = classify_changes(&candidates, &existing);

        if stored_project.is_some() && new_files == 0 && changed_files == 0 && deleted_files == 0 {
            return Ok(IndexResult {
                status: IndexStatus::Unchanged,
                counts: self.store.counts(&project.id)?,
                project,
                discovered_files: candidates.len(),
                new_files,
                changed_files,
                deleted_files,
                unchanged_files,
                parsed_files: 0,
                reused_files: unchanged_files,
                diagnostics: Vec::new(),
                warnings: report.warnings,
            });
        }

        // Hybrid resolution depends on pending facts from every source file. Reparse the
        // discovered set whenever the project changes so calls in otherwise unchanged files
        // cannot retain stale targets after definitions move, disappear, or become ambiguous.
        let parsed_files = parse_inputs.len();
        let parsed = self.parse_candidates(candidates.clone())?;
        let mut parsed_by_path = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for extracted in parsed {
            if let Some(file_diagnostics) = &extracted.diagnostics {
                diagnostics.push(file_diagnostics.clone());
            }
            parsed_by_path.insert(extracted.record.id.path.clone(), extracted);
        }
        diagnostics.sort_by(|left, right| left.path.cmp(&right.path));
        let rejects_existing_graph = diagnostics
            .iter()
            .any(|diagnostic| existing.contains_key(&diagnostic.path));
        if rejects_existing_graph {
            return Ok(IndexResult {
                status: IndexStatus::RejectedSyntax,
                counts: self.store.counts(&project.id)?,
                project,
                discovered_files: discovered_paths.len(),
                new_files,
                changed_files,
                deleted_files,
                unchanged_files,
                parsed_files,
                reused_files: existing_files.len(),
                diagnostics,
                warnings: report.warnings,
            });
        }
        let records = candidates
            .into_iter()
            .map(|candidate| candidate.record)
            .collect::<Vec<_>>();
        let (files, nodes, edges) =
            self.assemble_project_graph(&project, records, &mut parsed_by_path)?;
        self.ensure_not_cancelled()?;
        let outcome = self
            .store
            .replace_project_graph(&project, files, nodes, edges)?;
        project.generation = outcome.generation;
        let counts = self.store.counts(&project.id)?;
        Ok(IndexResult {
            status: IndexStatus::Indexed,
            project,
            discovered_files: discovered_paths.len(),
            new_files,
            changed_files,
            deleted_files,
            unchanged_files,
            parsed_files,
            reused_files: unchanged_files,
            counts,
            diagnostics,
            warnings: report.warnings,
        })
    }

    /// Refreshes one project-relative file while preserving all other committed file graphs.
    ///
    /// A malformed-source result leaves the prior project generation unchanged.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unknown projects, discovery/I/O failures, invalid graph facts,
    /// cancellation, or atomic persistence failure.
    pub fn refresh_file(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<FileRefreshResult, IndexError> {
        self.ensure_not_cancelled()?;
        let mut project = self
            .store
            .get_project(project_id)?
            .ok_or_else(|| goldeneye_store::StoreError::ProjectNotFound(project_id.clone()))?;
        let report = discover(Path::new(&project.root_path), &self.options.discovery)?;
        self.enforce_file_limit(report.files.len())?;
        let discovered = report
            .files
            .iter()
            .find(|file| relative_path(&file.relative_path).is_ok_and(|value| value == *path));
        let existing_files = self.store.list_files(project_id)?;
        let existing_target = existing_files
            .iter()
            .find(|file| file.id.path == *path)
            .cloned();

        let Some(discovered) = discovered else {
            if existing_target.is_none() {
                return self.refresh_result(
                    project_id,
                    path,
                    FileRefreshStatus::Unchanged,
                    project.generation,
                    Vec::new(),
                );
            }
            let records = existing_files
                .into_iter()
                .filter(|file| file.id.path != *path)
                .collect::<Vec<_>>();
            let mut parsed =
                self.parse_matching_records(&report.files, &records, project_id, None)?;
            let (files, nodes, edges) =
                self.assemble_project_graph(&project, records, &mut parsed)?;
            self.ensure_not_cancelled()?;
            let outcome = self
                .store
                .replace_project_graph(&project, files, nodes, edges)?;
            project.generation = outcome.generation;
            return self.refresh_result(
                project_id,
                path,
                FileRefreshStatus::Deleted,
                project.generation,
                Vec::new(),
            );
        };

        let candidate = Self::read_candidate(discovered, project_id)?;
        if existing_target
            .as_ref()
            .is_some_and(|file| file.content_hash == candidate.record.content_hash)
        {
            return self.refresh_result(
                project_id,
                path,
                FileRefreshStatus::Unchanged,
                project.generation,
                Vec::new(),
            );
        }
        let mut extracted = self
            .parse_candidates(vec![candidate])?
            .into_iter()
            .next()
            .ok_or(IndexError::MissingWorkerResult)?;
        if let Some(diagnostics) = extracted.diagnostics.take() {
            return self.refresh_result(
                project_id,
                path,
                FileRefreshStatus::RejectedSyntax,
                project.generation,
                vec![diagnostics],
            );
        }

        let mut records = existing_files
            .into_iter()
            .filter(|file| file.id.path != *path)
            .collect::<Vec<_>>();
        records.push(extracted.record.clone());
        let mut parsed =
            self.parse_matching_records(&report.files, &records, project_id, Some(path))?;
        parsed.insert(path.clone(), extracted);
        let (files, nodes, edges) = self.assemble_project_graph(&project, records, &mut parsed)?;
        self.ensure_not_cancelled()?;
        let outcome = self
            .store
            .replace_project_graph(&project, files, nodes, edges)?;
        project.generation = outcome.generation;
        self.refresh_result(
            project_id,
            path,
            FileRefreshStatus::Updated,
            project.generation,
            Vec::new(),
        )
    }

    fn refresh_result(
        &self,
        project: &ProjectId,
        path: &ProjectRelativePath,
        status: FileRefreshStatus,
        generation: Generation,
        diagnostics: Vec<FileSyntaxDiagnostics>,
    ) -> Result<FileRefreshResult, IndexError> {
        Ok(FileRefreshResult {
            project: project.clone(),
            path: path.clone(),
            status,
            generation,
            counts: self.store.counts(project)?,
            diagnostics,
        })
    }

    fn assemble_project_graph(
        &self,
        project: &ProjectRecord,
        mut files: Vec<FileRecord>,
        parsed: &mut BTreeMap<ProjectRelativePath, ExtractedFile>,
    ) -> Result<ProjectGraph, IndexError> {
        files.sort_by(|left, right| left.id.path.cmp(&right.id.path));
        let project_graph_node = project_node(project)?;
        let branch = branch_node(project)?;
        let mut edges = vec![project_has_branch(&project.id, &branch)?];
        let mut nodes = vec![project_graph_node, branch];
        let mut pending_calls = Vec::new();
        let mut pending_relations = Vec::new();
        let mut pending_imports = Vec::new();
        for file in &files {
            let graph = if let Some(mut extracted) = parsed.remove(&file.id.path) {
                pending_calls.append(&mut extracted.calls);
                pending_relations.append(&mut extracted.relations);
                pending_imports.append(&mut extracted.imports);
                FileGraph {
                    nodes: extracted.nodes,
                    edges: extracted.edges,
                }
            } else {
                self.reuse_file_graph(&file.id)?
            };
            let file_node = graph
                .nodes
                .iter()
                .find(|node| node.label.as_str() == "File")
                .ok_or_else(|| IndexError::MissingFileNode(file.id.path.clone()))?;
            edges.push(project_contains_file(&project.id, file_node)?);
            nodes.extend(graph.nodes);
            edges.extend(graph.edges);
        }
        let node_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        edges.retain(|edge| node_ids.contains(&edge.source) && node_ids.contains(&edge.target));
        crate::hybrid::resolve_project(
            &project.id,
            &nodes,
            &mut edges,
            pending_calls,
            pending_relations,
            pending_imports,
        )?;
        Ok((files, nodes, edges))
    }

    fn reuse_file_graph(&self, file: &FileId) -> Result<FileGraph, IndexError> {
        let nodes = self.store.nodes_for_file(file)?;
        let node_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let mut identities = BTreeSet::new();
        let mut edges = Vec::new();
        for node in &nodes {
            for edge in self.store.edges_from(&file.project, &node.id)? {
                if !node_ids.contains(&edge.source) {
                    continue;
                }
                let identity = (
                    edge.source.clone(),
                    edge.target.clone(),
                    edge.kind.clone(),
                    edge.discriminator.clone(),
                );
                if identities.insert(identity) {
                    edges.push(edge);
                }
            }
        }
        Ok(FileGraph { nodes, edges })
    }

    fn parse_matching_records(
        &self,
        discovered: &[DiscoveredFile],
        records: &[FileRecord],
        project: &ProjectId,
        excluded: Option<&ProjectRelativePath>,
    ) -> Result<BTreeMap<ProjectRelativePath, ExtractedFile>, IndexError> {
        let discovered_by_path = discovered
            .iter()
            .filter_map(|file| {
                relative_path(&file.relative_path)
                    .ok()
                    .map(|path| (path, file))
            })
            .collect::<BTreeMap<_, _>>();
        let records_by_path = records
            .iter()
            .map(|record| (record.id.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for (path, record) in records_by_path {
            if excluded.is_some_and(|excluded| excluded == &path) {
                continue;
            }
            let Some(discovered) = discovered_by_path.get(&path) else {
                continue;
            };
            let candidate = Self::read_candidate(discovered, project)?;
            if candidate.record.content_hash == record.content_hash {
                candidates.push(candidate);
            }
        }
        let mut parsed = BTreeMap::new();
        for extracted in self.parse_candidates(candidates)? {
            if extracted.diagnostics.is_none() {
                parsed.insert(extracted.record.id.path.clone(), extracted);
            }
        }
        Ok(parsed)
    }

    fn read_candidates(
        &self,
        files: &[DiscoveredFile],
        project: &ProjectId,
    ) -> Result<Vec<Candidate>, IndexError> {
        let supported_ids = self.provider.supported_ids();
        files
            .iter()
            .filter(|file| supported_ids.contains(&file.language))
            .map(|file| {
                self.ensure_not_cancelled()?;
                Self::read_candidate(file, project)
            })
            .collect()
    }

    fn read_candidate(file: &DiscoveredFile, project: &ProjectId) -> Result<Candidate, IndexError> {
        let source = fs::read(&file.absolute_path).map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let metadata = fs::metadata(&file.absolute_path).map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let modified = metadata.modified().map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let modified_ns = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let modified_ns = u64::try_from(modified_ns).map_err(|_| IndexError::MetadataOverflow {
            path: file.absolute_path.clone(),
            field: "modified_ns",
        })?;
        let byte_len = u64::try_from(source.len()).map_err(|_| IndexError::MetadataOverflow {
            path: file.absolute_path.clone(),
            field: "byte_len",
        })?;
        let path = relative_path(&file.relative_path)?;
        Ok(Candidate {
            record: FileRecord::new(
                FileId::new(project.clone(), path),
                ContentHash::of(&source),
                Generation::new(0),
                modified_ns,
                byte_len,
            ),
            language: file.language.clone(),
            source: Arc::from(source),
        })
    }

    fn parse_candidates(
        &self,
        candidates: Vec<Candidate>,
    ) -> Result<Vec<ExtractedFile>, IndexError> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let candidates = Arc::new(candidates);
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        let workers = self.options.max_workers.get().min(candidates.len());
        let mode = self.options.discovery.mode;
        std::thread::scope(|scope| -> Result<(), IndexError> {
            let mut handles = Vec::with_capacity(workers);
            for _ in 0..workers {
                let sender = sender.clone();
                let candidates = Arc::clone(&candidates);
                let provider = self.provider.clone();
                let cancellation = self.options.cancellation.clone();
                let next = &next;
                handles.push(scope.spawn(move || {
                    loop {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(candidate) = candidates.get(index).cloned() else {
                            break;
                        };
                        if sender
                            .send((index, extract(provider.clone(), candidate, mode)))
                            .is_err()
                        {
                            break;
                        }
                    }
                }));
            }
            drop(sender);
            for handle in handles {
                handle.join().map_err(|_| IndexError::WorkerPanicked)?;
            }
            Ok(())
        })?;
        self.ensure_not_cancelled()?;
        let mut results = receiver.into_iter().collect::<Vec<_>>();
        results.sort_by_key(|(index, _)| *index);
        results.into_iter().map(|(_, result)| result).collect()
    }

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
