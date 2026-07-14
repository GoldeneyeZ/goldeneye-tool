#![forbid(unsafe_code)]

//! Tool-neutral read/query services over Goldeneye's graph store.

mod ast_profile;
mod cypher;
mod engine;
mod semantic;
mod types;

pub use ast_profile::{
    AST_PROFILE_DIMS, AST_PROFILE_MAX_ENCODED_LEN, AstProfile, AstProfileParseError,
};
pub use engine::QueryEngine;
pub use semantic::{
    PRETRAINED_DIM, PRETRAINED_TOKEN_COUNT, PRETRAINED_TOKENS_SHA256,
    PRETRAINED_VECTOR_SHA256, PretrainedModel, PretrainedModelError, SEMANTIC_DENOMINATOR_EPSILON,
    SEMANTIC_DIM, SEMANTIC_EDGE_THRESHOLD, SEMANTIC_MAX_EDGES, SEMANTIC_MAX_OCCURRENCES,
    SEMANTIC_SPARSE_NON_ZERO, SEMANTIC_WINDOW, SemanticVector, cosine, module_proximity,
    tokenize_identifier,
};
pub use types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, EdgeSummary, GraphSchemaRequest, GraphSchemaResult,
    IndexStatusRequest, IndexStatusResult, NodeSummary, PageRequest, ProjectSummary, QueryError,
    QueryGraphRequest, QueryGraphResult, QueryValue, SchemaEntry, SearchGraphPage,
    SearchGraphRequest, TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};
