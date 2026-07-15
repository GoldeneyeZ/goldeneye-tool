//! Application-owned interfaces for external mechanisms.

mod crosslink;
mod error;

pub use crosslink::CrossLinkRepository;
pub use error::PortError;
