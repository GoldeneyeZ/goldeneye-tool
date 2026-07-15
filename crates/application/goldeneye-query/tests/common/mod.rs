use std::{collections::BTreeMap, fs, path::PathBuf};

use goldeneye_domain::{
    ByteSpan, ContentHash, EdgeKind, FileId, FileRecord, Generation, GraphEdge, GraphNode,
    GraphProperties, NodeId, NodeLabel, ProjectId, ProjectRecord, ProjectRelativePath,
    QualifiedName, SourcePoint, SourceSpan,
};
use goldeneye_query::QueryEngine;
use goldeneye_store::Store;
use serde_json::json;
use tempfile::TempDir;

#[allow(dead_code)]
pub struct Fixture {
    _temp: TempDir,
    pub database: PathBuf,
    pub project: ProjectId,
    pub root: PathBuf,
    pub source: String,
}

impl Fixture {
    #[allow(clippy::too_many_lines)]
    pub fn seeded() -> Self {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let root = temp.path().join("workspace");
        let source_path = root.join("src/lib.rs");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("create source");
        let source = concat!(
            "pub fn Alpha() { beta(); }\n",
            "pub fn beta() { Alpha(); }\n",
            "pub struct Café;\n",
            "impl Café { pub fn run() { beta(); } }\n",
            "pub fn run() {}\n",
            "pub fn main() { Alpha(); }\n",
        )
        .to_owned();
        fs::write(&source_path, source.as_bytes()).expect("write source fixture");

        let project = ProjectId::new("demo").expect("project ID");
        let project_record =
            ProjectRecord::new(project.clone(), root.to_string_lossy().into_owned())
                .expect("project record");
        let path = ProjectRelativePath::new("src/lib.rs").expect("relative path");
        let file = FileRecord::new(
            FileId::new(project.clone(), path.clone()),
            ContentHash::of(source.as_bytes()),
            Generation::new(0),
            1,
            u64::try_from(source.len()).expect("source length"),
        );

        let mut nodes = vec![
            node(
                &project,
                "module",
                "Module",
                "lib",
                "demo.src.lib",
                &path,
                &source,
                "pub fn Alpha",
            ),
            node(
                &project,
                "alpha",
                "Function",
                "Alpha",
                "demo.src.lib.Alpha",
                &path,
                &source,
                "pub fn Alpha() { beta(); }",
            ),
            node(
                &project,
                "beta",
                "Function",
                "beta",
                "demo.src.lib.beta",
                &path,
                &source,
                "pub fn beta() { Alpha(); }",
            ),
            node(
                &project,
                "cafe",
                "Struct",
                "Café",
                "demo.src.lib.Café",
                &path,
                &source,
                "pub struct Café;",
            ),
            node(
                &project,
                "method-run",
                "Method",
                "run",
                "demo.src.lib.Café.run",
                &path,
                &source,
                "pub fn run() { beta(); }",
            ),
            node(
                &project,
                "free-run",
                "Function",
                "run",
                "demo.src.lib.run",
                &path,
                &source,
                "pub fn run() {}",
            ),
            node(
                &project,
                "main",
                "Function",
                "main",
                "demo.src.lib.main",
                &path,
                &source,
                "pub fn main() { Alpha(); }",
            ),
        ];
        nodes[0].source_span = Some(whole_span(&source));
        nodes[6]
            .properties
            .insert("is_entry_point".to_owned(), json!(true));

        let edges = vec![
            edge(&project, "module", "alpha", "DEFINES"),
            edge(&project, "module", "beta", "DEFINES"),
            edge(&project, "module", "cafe", "DEFINES"),
            edge(&project, "cafe", "method-run", "DEFINES"),
            edge(&project, "module", "free-run", "DEFINES"),
            edge(&project, "module", "main", "DEFINES"),
            edge(&project, "alpha", "beta", "CALLS"),
            edge(&project, "beta", "alpha", "CALLS"),
            edge(&project, "method-run", "beta", "CALLS"),
            edge(&project, "main", "alpha", "CALLS"),
        ];

        let database = temp.path().join("graph.sqlite3");
        let mut store = Store::open(&database).expect("open graph store");
        store
            .replace_project_graph(&project_record, vec![file], nodes, edges)
            .expect("seed graph");
        drop(store);

        Self {
            _temp: temp,
            database,
            project,
            root,
            source,
        }
    }

    pub fn engine(&self) -> QueryEngine {
        QueryEngine::new(
            Store::open_read_only(&self.database).expect("open read-only query repository"),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn node(
    project: &ProjectId,
    id: &str,
    label: &str,
    name: &str,
    qualified_name: &str,
    path: &ProjectRelativePath,
    source: &str,
    needle: &str,
) -> GraphNode {
    let mut properties = GraphProperties::new();
    properties.insert("language".to_owned(), json!("rust"));
    GraphNode::new(
        project.clone(),
        NodeId::new(id).expect("node ID"),
        NodeLabel::new(label).expect("node label"),
        name,
        QualifiedName::new(qualified_name).expect("qualified name"),
        Some(path.clone()),
        Some(span_for(source, needle)),
        Generation::new(0),
    )
    .expect("graph node")
    .with_properties(properties)
}

fn edge(project: &ProjectId, source: &str, target: &str, kind: &str) -> GraphEdge {
    GraphEdge::new(
        project.clone(),
        NodeId::new(source).expect("source ID"),
        NodeId::new(target).expect("target ID"),
        EdgeKind::new(kind).expect("edge kind"),
        Generation::new(0),
    )
}

fn span_for(source: &str, needle: &str) -> SourceSpan {
    let start = source.find(needle).expect("needle in fixture");
    let end = start + needle.len();
    let start_prefix = &source[..start];
    let end_prefix = &source[..end];
    SourceSpan::new(
        ByteSpan::new(
            u64::try_from(start).expect("start byte"),
            u64::try_from(end).expect("end byte"),
        )
        .expect("byte span"),
        point(start_prefix),
        point(end_prefix),
    )
    .expect("source span")
}

fn whole_span(source: &str) -> SourceSpan {
    SourceSpan::new(
        ByteSpan::new(0, u64::try_from(source.len()).expect("source length")).expect("span"),
        SourcePoint::new(0, 0),
        point(source),
    )
    .expect("whole source span")
}

fn point(prefix: &str) -> SourcePoint {
    let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len(), |(_, tail)| tail.len());
    SourcePoint::new(
        u64::try_from(row).expect("row"),
        u64::try_from(column).expect("column"),
    )
}

#[allow(dead_code)]
pub fn properties(entries: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}
