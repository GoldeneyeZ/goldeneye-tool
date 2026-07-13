#![forbid(unsafe_code)]

//! Tool-neutral read/query services over Goldeneye's graph store.

mod cypher;
mod engine;
mod types;

pub use engine::QueryEngine;
pub use types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, EdgeSummary, GraphSchemaRequest, GraphSchemaResult,
    IndexStatusRequest, IndexStatusResult, NodeSummary, PageRequest, ProjectSummary, QueryError,
    QueryGraphRequest, QueryGraphResult, QueryValue, SchemaEntry, SearchGraphPage,
    SearchGraphRequest, TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};
