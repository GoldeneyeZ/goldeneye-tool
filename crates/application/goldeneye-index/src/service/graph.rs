use super::{
    Arc, BTreeMap, BTreeSet, ExtractedFile, FileGraph, FileRecord, IndexError, IndexRepository,
    IndexService, ProjectGraph, ProjectRecord, ProjectRelativePath, branch_node,
    deduplicate_shared_modules, project_contains_file, project_has_branch, project_node,
};

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    pub(super) fn assemble_project_graph(
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
        let mut source_files = Vec::new();
        for file in &files {
            let graph = if let Some(mut extracted) = parsed.remove(&file.id.path) {
                source_files.push(crate::enrichment::SourceFile {
                    path: extracted.record.id.path.clone(),
                    source: Arc::clone(&extracted.source),
                });
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
        deduplicate_shared_modules(&mut nodes);
        let node_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        edges.retain(|edge| node_ids.contains(&edge.source) && node_ids.contains(&edge.target));
        crate::hybrid::resolve_project(
            &project.id,
            &nodes,
            &mut edges,
            pending_calls.clone(),
            pending_relations,
            pending_imports.clone(),
        )?;
        crate::enrichment::apply_project(
            &project.id,
            &mut nodes,
            &mut edges,
            &pending_calls,
            &pending_imports,
            &source_files,
        )?;
        Ok((files, nodes, edges))
    }
}
