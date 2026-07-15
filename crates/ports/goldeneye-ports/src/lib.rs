//! Application-owned interfaces for external mechanisms.

mod crosslink;
mod error;
mod query;

pub use crosslink::CrossLinkRepository;
pub use error::PortError;
pub use query::{
    ConnectionSettings, GraphCounts, NodeSignatureRecord, NodeVectorRecord, QueryRepository,
    STORED_VECTOR_DIM, SchemaInfo, SearchHit, StoredVector, TokenVectorRecord,
};
