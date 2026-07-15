use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use goldeneye_domain::{FileRecord, Generation, GraphEdge, GraphNode, NodeId};

use crate::types::{NodeSummary, SearchGraphPage, TracePathResult};

use super::{
    architecture::ArchitectureSummary,
    search::{MAX_CACHED_SEARCH_PAGES, SearchCacheKey},
    trace::{MAX_CACHED_TRACE_RESULTS, TraceCacheKey},
};

pub(super) struct ProjectGraph {
    pub(super) generation: u64,
    pub(super) nodes: Vec<GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
    pub(super) degrees: BTreeMap<NodeId, (usize, usize)>,
    pub(super) edges_by_node: BTreeMap<NodeId, Vec<usize>>,
    pub(super) define_counts: BTreeMap<NodeId, usize>,
    pub(super) nodes_by_name: BTreeMap<String, Vec<usize>>,
    pub(super) nodes_by_id: BTreeMap<NodeId, usize>,
    pub(super) nodes_by_qualified_name: BTreeMap<String, usize>,
    search_pages: Mutex<BTreeMap<SearchCacheKey, SearchGraphPage>>,
    trace_results: Mutex<BTreeMap<TraceCacheKey, TracePathResult>>,
    files_by_path: Mutex<BTreeMap<String, FileRecord>>,
    architecture_summary: OnceLock<ArchitectureSummary>,
}

struct GraphIndexes {
    edges_by_node: BTreeMap<NodeId, Vec<usize>>,
    define_counts: BTreeMap<NodeId, usize>,
    nodes_by_name: BTreeMap<String, Vec<usize>>,
    nodes_by_id: BTreeMap<NodeId, usize>,
    nodes_by_qualified_name: BTreeMap<String, usize>,
}

impl ProjectGraph {
    pub(super) fn new(
        generation: Generation,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Self {
        let degrees = degrees(&edges);
        let indexes = GraphIndexes::new(&nodes, &edges);
        Self {
            generation: generation.value(),
            nodes,
            edges,
            degrees,
            edges_by_node: indexes.edges_by_node,
            define_counts: indexes.define_counts,
            nodes_by_name: indexes.nodes_by_name,
            nodes_by_id: indexes.nodes_by_id,
            nodes_by_qualified_name: indexes.nodes_by_qualified_name,
            search_pages: Mutex::new(BTreeMap::new()),
            trace_results: Mutex::new(BTreeMap::new()),
            files_by_path: Mutex::new(BTreeMap::new()),
            architecture_summary: OnceLock::new(),
        }
    }

    pub(super) fn cached_file(&self, path: &str) -> Option<FileRecord> {
        self.files_by_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
            .cloned()
    }

    pub(super) fn node(&self, id: &NodeId) -> Option<&GraphNode> {
        self.nodes_by_id
            .get(id)
            .and_then(|index| self.nodes.get(*index))
    }

    pub(super) fn cache_file(&self, file: FileRecord) {
        self.files_by_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(file.id.path.as_str().to_owned(), file);
    }

    pub(super) fn cached_search(&self, key: &SearchCacheKey) -> Option<SearchGraphPage> {
        self.search_pages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    pub(super) fn cache_search(&self, key: SearchCacheKey, page: SearchGraphPage) {
        cache_bounded(&self.search_pages, MAX_CACHED_SEARCH_PAGES, key, page);
    }

    pub(super) fn cached_trace(&self, key: &TraceCacheKey) -> Option<TracePathResult> {
        self.trace_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    pub(super) fn cache_trace(&self, key: TraceCacheKey, result: TracePathResult) {
        cache_bounded(&self.trace_results, MAX_CACHED_TRACE_RESULTS, key, result);
    }

    pub(super) fn architecture_summary(&self) -> &ArchitectureSummary {
        self.architecture_summary
            .get_or_init(|| ArchitectureSummary::from_graph(self))
    }
}

impl GraphIndexes {
    fn new(nodes: &[GraphNode], edges: &[GraphEdge]) -> Self {
        let (nodes_by_name, nodes_by_id, nodes_by_qualified_name) = index_nodes(nodes);
        let (edges_by_node, define_counts) = index_edges(edges);
        Self {
            edges_by_node,
            define_counts,
            nodes_by_name,
            nodes_by_id,
            nodes_by_qualified_name,
        }
    }
}

type NodeIndexes = (
    BTreeMap<String, Vec<usize>>,
    BTreeMap<NodeId, usize>,
    BTreeMap<String, usize>,
);

fn index_nodes(nodes: &[GraphNode]) -> NodeIndexes {
    let mut nodes_by_name = BTreeMap::<String, Vec<usize>>::new();
    let mut nodes_by_id = BTreeMap::<NodeId, usize>::new();
    let mut nodes_by_qualified_name = BTreeMap::<String, usize>::new();
    for (index, node) in nodes.iter().enumerate() {
        nodes_by_name
            .entry(node.name.clone())
            .or_default()
            .push(index);
        nodes_by_id.insert(node.id.clone(), index);
        nodes_by_qualified_name.insert(node.qualified_name.as_str().to_owned(), index);
    }
    (nodes_by_name, nodes_by_id, nodes_by_qualified_name)
}

fn index_edges(edges: &[GraphEdge]) -> (BTreeMap<NodeId, Vec<usize>>, BTreeMap<NodeId, usize>) {
    let mut edges_by_node = BTreeMap::<NodeId, Vec<usize>>::new();
    let mut define_counts = BTreeMap::<NodeId, usize>::new();
    for (index, edge) in edges.iter().enumerate() {
        edges_by_node
            .entry(edge.source.clone())
            .or_default()
            .push(index);
        if edge.target != edge.source {
            edges_by_node
                .entry(edge.target.clone())
                .or_default()
                .push(index);
        }
        if edge.kind.as_str() == "DEFINES" {
            *define_counts.entry(edge.source.clone()).or_default() += 1;
        }
    }
    (edges_by_node, define_counts)
}

fn cache_bounded<K: Clone + Ord, V>(cache: &Mutex<BTreeMap<K, V>>, limit: usize, key: K, value: V) {
    let mut entries = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if entries.len() >= limit
        && !entries.contains_key(&key)
        && let Some(first) = entries.keys().next().cloned()
    {
        entries.remove(&first);
    }
    entries.insert(key, value);
}

pub(crate) fn degrees(edges: &[GraphEdge]) -> BTreeMap<NodeId, (usize, usize)> {
    let mut degrees = BTreeMap::new();
    for edge in edges {
        degrees.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
        degrees.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
    }
    degrees
}

pub(crate) fn node_summary(
    node: &GraphNode,
    rank: Option<f64>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    connected_names: Vec<String>,
) -> NodeSummary {
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or((0, 0));
    NodeSummary {
        id: node.id.as_str().to_owned(),
        name: node.name.clone(),
        qualified_name: node.qualified_name.as_str().to_owned(),
        label: node.label.as_str().to_owned(),
        file_path: node.file_path.as_ref().map(|path| path.as_str().to_owned()),
        start_byte: node.source_span.map(|span| span.bytes.start),
        end_byte: node.source_span.map(|span| span.bytes.end),
        start_line: node.source_span.map(|span| span.start.row + 1),
        end_line: node.source_span.map(|span| span.end.row + 1),
        generation: node.generation.value(),
        in_degree,
        out_degree,
        rank,
        connected_names,
        properties: node.properties.clone(),
    }
}
