#![forbid(unsafe_code)]

//! Tool-neutral read/query services over Goldeneye's graph store.

mod ast_profile;
mod cypher;
mod engine;
mod types;

pub use ast_profile::{
    AST_PROFILE_DIMS, AST_PROFILE_MAX_ENCODED_LEN, AstProfile, AstProfileParseError,
};
pub use engine::QueryEngine;
pub use types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, EdgeSummary, GraphSchemaRequest, GraphSchemaResult,
    IndexStatusRequest, IndexStatusResult, NodeSummary, PageRequest, ProjectSummary, QueryError,
    QueryGraphRequest, QueryGraphResult, QueryValue, SchemaEntry, SearchGraphPage,
    SearchGraphRequest, TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};
