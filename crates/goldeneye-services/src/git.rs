use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use goldeneye_domain::{FileId, NodeId, ProjectId, ProjectRelativePath};
use goldeneye_git::{
    DetectChangesOptions, GitCoChange, GitFileHistory, GitLimits, collect_history, detect_changes,
    resolve_context,
};
use goldeneye_store::{GitCoChangeRecord, GitFileHistoryRecord, Store};
use serde::{Deserialize, Serialize};

use crate::{CancellationToken, QueryError, ServiceError, Services};

pub const DEFAULT_CHANGE_DEPTH: usize = 2;
pub const MAX_CHANGE_DEPTH: usize = 15;
pub const MAX_IMPACTED_SYMBOLS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectChangesRequest {
    pub project: ProjectId,
    pub scope: Option<String>,
    pub depth: usize,
    pub base_branch: String,
    pub since: Option<String>,
}

impl DetectChangesRequest {
    #[must_use]
    pub fn new(project: ProjectId) -> Self {
        Self {
            project,
            scope: None,
            depth: DEFAULT_CHANGE_DEPTH,
            base_branch: "main".to_owned(),
            since: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactedSymbol {
    pub name: String,
    pub label: String,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectChangesResult {
    pub changed_files: Vec<String>,
    pub changed_count: usize,
    pub impacted_symbols: Vec<ImpactedSymbol>,
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip)]
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHistoryResult {
    pub files: usize,
    pub couplings: usize,
    pub enriched_files: usize,
    pub enriched_edges: usize,
}

impl Services {
    /// Resolves audited Git/worktree context for an indexed project.
    ///
    /// # Errors
    ///
    /// Returns a typed project, path-policy, cancellation, or Git error.
    pub fn git_context(
        &self,
        project: &ProjectId,
        cancellation: &CancellationToken,
    ) -> Result<goldeneye_git::GitContext, ServiceError> {
        let (_, root) = self.project_store_and_root(project)?;
        let cancelled = || cancellation.is_cancelled();
        Ok(resolve_context(&root, &cancelled, &GitLimits::default())?)
    }

    /// Recomputes bounded Git history and atomically refreshes graph enrichment.
    ///
    /// # Errors
    ///
    /// Returns a typed project, path-policy, cancellation, Git, validation, or storage error.
    pub fn refresh_git_history(
        &self,
        project: &ProjectId,
        cancellation: &CancellationToken,
    ) -> Result<GitHistoryResult, ServiceError> {
        let (_, root) = self.project_store_and_root(project)?;
        self.refresh_git_history_at(project, &root, cancellation)
    }

    pub(crate) fn refresh_git_history_at(
        &self,
        project: &ProjectId,
        root: &Path,
        cancellation: &CancellationToken,
    ) -> Result<GitHistoryResult, ServiceError> {
        let cancelled = || cancellation.is_cancelled();
        let history = collect_history(root, &cancelled, &GitLimits::default())?;
        let files = history
            .files
            .iter()
            .filter_map(convert_file_history)
            .collect::<Vec<_>>();
        let couplings = history
            .couplings
            .iter()
            .filter_map(convert_cochange)
            .collect::<Vec<_>>();
        self.prepare_database()?;
        let mut store = Store::open(self.config().database_path())?;
        let outcome = store.replace_git_history(project, &files, &couplings)?;
        Ok(GitHistoryResult {
            files: outcome.files,
            couplings: outcome.couplings,
            enriched_files: outcome.enriched_files,
            enriched_edges: outcome.enriched_edges,
        })
    }

    /// Detects committed, dirty, staged, renamed, and untracked paths and computes impact.
    ///
    /// # Errors
    ///
    /// Returns a typed project, path-policy, cancellation, Git, or storage error.
    pub fn detect_changes(
        &self,
        request: &DetectChangesRequest,
        cancellation: &CancellationToken,
    ) -> Result<DetectChangesResult, ServiceError> {
        let reference = request
            .since
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&request.base_branch);
        goldeneye_git::validate_reference(reference)?;
        let (store, root) = self.project_store_and_root(&request.project)?;
        let cancelled = || cancellation.is_cancelled();
        let changes = detect_changes(
            &root,
            &DetectChangesOptions {
                base_branch: request.base_branch.clone(),
                since: request.since.clone(),
            },
            &cancelled,
            &GitLimits::default(),
        )?;
        let depth = request.depth.min(MAX_CHANGE_DEPTH);
        let wants_symbols = request
            .scope
            .as_deref()
            .is_none_or(|scope| matches!(scope, "symbols" | "impact"));
        let impacted_symbols = if wants_symbols {
            impacted_symbols(&store, &request.project, &changes.files, depth)?
        } else {
            Vec::new()
        };
        let hint = changes.failure.as_ref().map(|failure| {
            format!(
                "git diff exited with status {}. Check that branch '{}' exists.",
                failure.status, failure.reference
            )
        });
        Ok(DetectChangesResult {
            changed_count: changes.files.len(),
            changed_files: changes.files,
            impacted_symbols,
            depth,
            is_error: hint.is_some(),
            hint,
        })
    }

    fn project_store_and_root(
        &self,
        project: &ProjectId,
    ) -> Result<(Store, std::path::PathBuf), ServiceError> {
        self.prepare_database()?;
        let store = Store::open(self.config().database_path())?;
        let record = store
            .get_project(project)?
            .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))?;
        let root = self.resolve_repository(Path::new(&record.root_path))?;
        Ok((store, root))
    }
}

fn convert_file_history(file: &GitFileHistory) -> Option<GitFileHistoryRecord> {
    Some(GitFileHistoryRecord {
        path: ProjectRelativePath::new(&file.path).ok()?,
        change_count: file.change_count,
        last_modified: file.last_modified.max(0),
    })
}

fn convert_cochange(coupling: &GitCoChange) -> Option<GitCoChangeRecord> {
    let mut file_a = ProjectRelativePath::new(&coupling.file_a).ok()?;
    let mut file_b = ProjectRelativePath::new(&coupling.file_b).ok()?;
    if file_b < file_a {
        std::mem::swap(&mut file_a, &mut file_b);
    }
    (file_a != file_b).then_some(GitCoChangeRecord {
        file_a,
        file_b,
        co_changes: coupling.co_changes,
        coupling_score: coupling.coupling_score,
        last_co_change: coupling.last_co_change.max(0),
    })
}

fn impacted_symbols(
    store: &Store,
    project: &ProjectId,
    changed_files: &[String],
    depth: usize,
) -> Result<Vec<ImpactedSymbol>, ServiceError> {
    let mut symbols = BTreeMap::<String, ImpactedSymbol>::new();
    let mut visited = BTreeSet::<NodeId>::new();
    let mut queue = VecDeque::<(NodeId, usize)>::new();

    for changed in changed_files {
        let Ok(path) = ProjectRelativePath::new(changed) else {
            continue;
        };
        add_file_symbols(
            store,
            project,
            &path,
            0,
            &mut symbols,
            &mut visited,
            &mut queue,
        )?;
        if depth > 0 {
            for coupling in store.coupled_files(project, &path)? {
                let coupled = if coupling.file_a == path {
                    coupling.file_b
                } else {
                    coupling.file_a
                };
                add_file_symbols(
                    store,
                    project,
                    &coupled,
                    1,
                    &mut symbols,
                    &mut visited,
                    &mut queue,
                )?;
            }
        }
    }

    while let Some((node_id, level)) = queue.pop_front() {
        if level >= depth || symbols.len() >= MAX_IMPACTED_SYMBOLS {
            continue;
        }
        for edge in store.edges_to(project, &node_id)? {
            if !is_impact_edge(edge.kind.as_str()) || !visited.insert(edge.source.clone()) {
                continue;
            }
            let Some(node) = store.get_node(project, &edge.source)? else {
                continue;
            };
            if is_symbol_label(node.label.as_str()) {
                add_symbol(&node, &mut symbols);
            }
            queue.push_back((node.id, level + 1));
            if symbols.len() >= MAX_IMPACTED_SYMBOLS {
                break;
            }
        }
    }
    Ok(symbols.into_values().take(MAX_IMPACTED_SYMBOLS).collect())
}

#[allow(clippy::too_many_arguments)]
fn add_file_symbols(
    store: &Store,
    project: &ProjectId,
    path: &ProjectRelativePath,
    level: usize,
    symbols: &mut BTreeMap<String, ImpactedSymbol>,
    visited: &mut BTreeSet<NodeId>,
    queue: &mut VecDeque<(NodeId, usize)>,
) -> Result<(), ServiceError> {
    for node in store.nodes_for_file(&FileId::new(project.clone(), path.clone()))? {
        if !is_symbol_label(node.label.as_str()) || !visited.insert(node.id.clone()) {
            continue;
        }
        add_symbol(&node, symbols);
        queue.push_back((node.id, level));
        if symbols.len() >= MAX_IMPACTED_SYMBOLS {
            break;
        }
    }
    Ok(())
}

fn add_symbol(node: &goldeneye_domain::GraphNode, symbols: &mut BTreeMap<String, ImpactedSymbol>) {
    let file = node
        .file_path
        .as_ref()
        .map_or_else(String::new, |path| path.as_str().to_owned());
    symbols
        .entry(node.qualified_name.as_str().to_owned())
        .or_insert_with(|| ImpactedSymbol {
            name: node.name.clone(),
            label: node.label.as_str().to_owned(),
            file,
        });
}

fn is_symbol_label(label: &str) -> bool {
    !matches!(label, "File" | "Folder" | "Project")
}

fn is_impact_edge(kind: &str) -> bool {
    !matches!(
        kind,
        "DEFINES"
            | "DEFINES_METHOD"
            | "CONTAINS_FILE"
            | "CONTAINS_FOLDER"
            | "SIMILAR_TO"
            | "SEMANTICALLY_RELATED"
            | "FILE_CHANGES_WITH"
    )
}
