#![forbid(unsafe_code)]

//! Tool-neutral read/query services over Goldeneye's graph store.

mod ast_profile;
mod cypher;
mod engine;
mod rotsq;
mod semantic;
mod similarity;
mod types;

pub use ast_profile::{
    AST_PROFILE_DIMS, AST_PROFILE_MAX_ENCODED_LEN, AstProfile, AstProfileParseError,
};
pub use engine::QueryEngine;
pub use rotsq::{
    ROTSQ_BITS, ROTSQ_CODE_BYTES, ROTSQ_DIM, ROTSQ_INPUT_DIM, ROTSQ_LEVELS,
    RotatedScalarCode,
};
pub use semantic::{
    PRETRAINED_DIM, PRETRAINED_TOKEN_COUNT, PRETRAINED_TOKENS_SHA256, PRETRAINED_VECTOR_SHA256,
    PretrainedModel, PretrainedModelError, SEMANTIC_DENOMINATOR_EPSILON, SEMANTIC_DIM,
    SEMANTIC_EDGE_THRESHOLD, SEMANTIC_MAX_EDGES, SEMANTIC_MAX_OCCURRENCES,
    SEMANTIC_SPARSE_NON_ZERO, SEMANTIC_WINDOW, SemanticVector, cosine, module_proximity,
    tokenize_identifier,
};
pub use similarity::{
    LSH_BANDS, LSH_MAX_BUCKET_CANDIDATES, LSH_ROWS, MAX_STRUCTURAL_TOKENS, MINHASH_HEX_LEN,
    MINHASH_JACCARD_THRESHOLD, MINHASH_K, MINHASH_MAX_EDGES, MINHASH_MIN_NODES,
    MINHASH_MIN_UNIQUE_TRIGRAMS, MinHashDecodeError, MinHashSignature, SimHashSignature,
    normalize_leaf_kind,
};
pub use types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, EdgeSummary, GraphSchemaRequest, GraphSchemaResult,
    IndexStatusRequest, IndexStatusResult, NodeSummary, PageRequest, ProjectSummary, QueryError,
    QueryGraphRequest, QueryGraphResult, QueryValue, SchemaEntry, SearchGraphPage,
    SearchGraphRequest, TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};
