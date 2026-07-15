use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, NodeId, ProjectId,
    ProjectRelativePath,
};
use goldeneye_ports::{
    IndexExtractedCall as ExtractedCall, IndexExtractedImport as ExtractedImport,
    IndexExtractedRelation as ExtractedRelation,
};
use serde_json::{Value, json};

use super::{
    index::{DefinitionIndex, DefinitionRef, ResolutionStrategy},
    names::{is_lsp_wired, normalize_name},
};
use crate::IndexError;

const MAX_RESOLUTION_PASSES: usize = 3;
const MAX_PROJECT_PENDING_FACTS: usize = 100_000;

type EdgeIdentity = (NodeId, NodeId, String, String);
type ImportsByFile = BTreeMap<ProjectRelativePath, Vec<ExtractedImport>>;

enum ResolvedFact<T> {
    Added,
    Ignored,
    Unresolved(T),
}

struct ResolutionState<'a> {
    project: &'a ProjectId,
    index: &'a DefinitionIndex,
    imports_by_file: &'a ImportsByFile,
    identities: &'a mut BTreeSet<EdgeIdentity>,
    edges: &'a mut Vec<GraphEdge>,
}

pub(crate) fn resolve_project(
    project: &ProjectId,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
    mut calls: Vec<ExtractedCall>,
    mut relations: Vec<ExtractedRelation>,
    mut imports: Vec<ExtractedImport>,
) -> Result<(), IndexError> {
    normalize_pending_facts(&mut calls, &mut relations, &mut imports);
    remove_provisional_edges(edges, &calls, &relations);
    let index = DefinitionIndex::build(nodes);
    let imports_by_file = imports_by_file(imports);
    let mut identities = edge_identities(edges);
    ResolutionState {
        project,
        index: &index,
        imports_by_file: &imports_by_file,
        identities: &mut identities,
        edges,
    }
    .resolve(calls, relations)?;
    sort_edges(edges);
    Ok(())
}

fn normalize_pending_facts(
    calls: &mut Vec<ExtractedCall>,
    relations: &mut Vec<ExtractedRelation>,
    imports: &mut Vec<ExtractedImport>,
) {
    calls.sort_by(|left, right| {
        (&left.file, left.start_byte, &left.callee_name).cmp(&(
            &right.file,
            right.start_byte,
            &right.callee_name,
        ))
    });
    calls.dedup_by(|left, right| {
        left.file == right.file
            && left.start_byte == right.start_byte
            && left.callee_name == right.callee_name
    });
    relations.sort();
    relations.dedup();
    imports.sort();
    imports.dedup();
    calls.truncate(MAX_PROJECT_PENDING_FACTS);
    relations.truncate(MAX_PROJECT_PENDING_FACTS);
    imports.truncate(MAX_PROJECT_PENDING_FACTS);
}

fn remove_provisional_edges(
    edges: &mut Vec<GraphEdge>,
    calls: &[ExtractedCall],
    relations: &[ExtractedRelation],
) {
    let call_sites = provisional_call_sites(calls);
    let relation_sites = provisional_relation_sites(relations);
    edges.retain(|edge| {
        let provisional_call = edge.kind.as_str() == "CALLS"
            && call_sites.contains(&(edge.source.clone(), edge.discriminator.as_str().to_owned()));
        let provisional_relation = relation_sites.contains(&(
            edge.source.clone(),
            edge.kind.as_str().to_owned(),
            normalize_name(edge.discriminator.as_str()),
        ));
        !provisional_call && !provisional_relation
    });
}

fn provisional_call_sites(calls: &[ExtractedCall]) -> BTreeSet<(NodeId, String)> {
    calls
        .iter()
        .filter(|call| is_lsp_wired(call.language.as_str()))
        .map(|call| (call.source.clone(), call.start_byte.to_string()))
        .collect()
}

fn provisional_relation_sites(
    relations: &[ExtractedRelation],
) -> BTreeSet<(NodeId, String, String)> {
    relations
        .iter()
        .filter(|relation| {
            is_lsp_wired(relation.language.as_str()) || relation.language.as_str() == "graphql"
        })
        .map(|relation| {
            (
                relation.source.clone(),
                relation.kind.to_owned(),
                normalize_name(&relation.target_name),
            )
        })
        .collect()
}

fn imports_by_file(imports: Vec<ExtractedImport>) -> ImportsByFile {
    imports.into_iter().fold(
        BTreeMap::<ProjectRelativePath, Vec<ExtractedImport>>::new(),
        |mut by_file, import| {
            by_file.entry(import.file.clone()).or_default().push(import);
            by_file
        },
    )
}

fn edge_identities(edges: &[GraphEdge]) -> BTreeSet<EdgeIdentity> {
    edges
        .iter()
        .map(|edge| {
            (
                edge.source.clone(),
                edge.target.clone(),
                edge.kind.as_str().to_owned(),
                edge.discriminator.as_str().to_owned(),
            )
        })
        .collect()
}

impl ResolutionState<'_> {
    fn resolve(
        &mut self,
        calls: Vec<ExtractedCall>,
        relations: Vec<ExtractedRelation>,
    ) -> Result<(), IndexError> {
        let mut unresolved_calls = calls;
        let mut unresolved_relations = relations;
        for _ in 0..MAX_RESOLUTION_PASSES {
            let (next_calls, call_progress) = self.resolve_calls(unresolved_calls)?;
            unresolved_calls = next_calls;
            let (next_relations, relation_progress) =
                self.resolve_relations(unresolved_relations)?;
            unresolved_relations = next_relations;
            if call_progress + relation_progress == 0 {
                break;
            }
        }
        Ok(())
    }

    fn resolve_calls(
        &mut self,
        calls: Vec<ExtractedCall>,
    ) -> Result<(Vec<ExtractedCall>, usize), IndexError> {
        let mut unresolved = Vec::new();
        let mut progress = 0;
        for call in calls {
            match self.resolve_call(call)? {
                ResolvedFact::Added => progress += 1,
                ResolvedFact::Ignored => {}
                ResolvedFact::Unresolved(call) => unresolved.push(call),
            }
        }
        Ok((unresolved, progress))
    }

    fn resolve_call(
        &mut self,
        call: ExtractedCall,
    ) -> Result<ResolvedFact<ExtractedCall>, IndexError> {
        let file_imports = self.file_imports(&call.file);
        let Some((target, strategy)) = self.index.resolve_call(&call, file_imports) else {
            return Ok(ResolvedFact::Unresolved(call));
        };
        if call.source == target.id {
            return Ok(ResolvedFact::Ignored);
        }
        let discriminator = call.start_byte.to_string();
        let identity = (
            call.source.clone(),
            target.id.clone(),
            "CALLS".to_owned(),
            discriminator.clone(),
        );
        if !self.identities.insert(identity) {
            return Ok(ResolvedFact::Ignored);
        }
        self.edges.push(resolved_call_edge(
            self.project,
            &call,
            target,
            strategy,
            discriminator,
        )?);
        Ok(ResolvedFact::Added)
    }

    fn resolve_relations(
        &mut self,
        relations: Vec<ExtractedRelation>,
    ) -> Result<(Vec<ExtractedRelation>, usize), IndexError> {
        let mut unresolved = Vec::new();
        let mut progress = 0;
        for relation in relations {
            match self.resolve_relation(relation)? {
                ResolvedFact::Added => progress += 1,
                ResolvedFact::Ignored => {}
                ResolvedFact::Unresolved(relation) => unresolved.push(relation),
            }
        }
        Ok((unresolved, progress))
    }

    fn resolve_relation(
        &mut self,
        relation: ExtractedRelation,
    ) -> Result<ResolvedFact<ExtractedRelation>, IndexError> {
        let file_imports = self.file_imports(&relation.file);
        let Some(target) = self.index.resolve_relation(&relation, file_imports) else {
            return Ok(ResolvedFact::Unresolved(relation));
        };
        if relation.source == target.id {
            return Ok(ResolvedFact::Ignored);
        }
        let discriminator = normalize_name(&relation.target_name);
        let identity = (
            relation.source.clone(),
            target.id.clone(),
            relation.kind.to_owned(),
            discriminator.clone(),
        );
        if !self.identities.insert(identity) {
            return Ok(ResolvedFact::Ignored);
        }
        self.edges.push(graph_edge(
            self.project,
            relation.source.clone(),
            target.id.clone(),
            relation.kind,
            Some(discriminator),
            GraphProperties::new(),
        )?);
        Ok(ResolvedFact::Added)
    }

    fn file_imports(&self, file: &ProjectRelativePath) -> &[ExtractedImport] {
        self.imports_by_file
            .get(file)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

fn resolved_call_edge(
    project: &ProjectId,
    call: &ExtractedCall,
    target: &DefinitionRef,
    strategy: ResolutionStrategy,
    discriminator: String,
) -> Result<GraphEdge, IndexError> {
    let mut properties = GraphProperties::new();
    properties.insert("callee".into(), Value::String(call.callee_name.clone()));
    properties.insert("line".into(), json!(call.line));
    properties.insert(
        "resolved_qn".into(),
        Value::String(target.qualified_name.clone()),
    );
    properties.insert(
        "strategy".into(),
        Value::String(strategy.as_str().to_owned()),
    );
    graph_edge(
        project,
        call.source.clone(),
        target.id.clone(),
        "CALLS",
        Some(discriminator),
        properties,
    )
}

fn graph_edge(
    project: &ProjectId,
    source: NodeId,
    target: NodeId,
    kind: &str,
    discriminator: Option<String>,
    properties: GraphProperties,
) -> Result<GraphEdge, IndexError> {
    let edge = GraphEdge::new(
        project.clone(),
        source,
        target,
        EdgeKind::new(kind)?,
        Generation::new(0),
    )
    .with_properties(properties);
    match discriminator {
        Some(value) => edge.with_discriminator(value).map_err(IndexError::from),
        None => Ok(edge),
    }
}

fn sort_edges(edges: &mut [GraphEdge]) {
    edges.sort_by(|left, right| {
        (
            &left.source,
            left.kind.as_str(),
            &left.target,
            left.discriminator.as_str(),
        )
            .cmp(&(
                &right.source,
                right.kind.as_str(),
                &right.target,
                right.discriminator.as_str(),
            ))
    });
}
