//! Tree-sitter syntax services for Goldeneye.

mod edit_port;
mod engine;
#[cfg(feature = "full-grammar-pack")]
mod full_grammar;
mod grammar;
mod inspect;
mod locator;

pub use engine::{
    DiagnosticKind, MAX_DIAGNOSTIC_DETAILS, ReparseResult, SyntaxDiagnostic, SyntaxEdit,
    SyntaxEngine, SyntaxSnapshot,
};
#[cfg(feature = "full-grammar-pack")]
pub use full_grammar::FullGrammarProvider;
pub use goldeneye_grammar_pack::{
    GrammarPackLock, GrammarPackState, GrammarRecord, LanguageBindingStatus, LanguageMapping,
    PackError, VerifiedPack, hash_grammar_assets, lock_file_hash, verify_materialized_pack,
};
#[cfg(feature = "core-grammars")]
pub use grammar::CoreGrammarProvider;
pub use grammar::{
    EditContentRegion, EditPointKind, Grammar, GrammarProvider, GrammarSource, SyntaxError,
};
pub use inspect::{
    DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES, DEFAULT_PREVIEW_CHARS, InspectError, InspectRequest,
    MAX_INSPECT_DEPTH, MAX_INSPECT_KIND_FILTERS, MAX_INSPECT_NODES, MAX_PREVIEW_CHARS,
    SyntaxInspection, SyntaxNodeView, inspect_syntax,
};
pub use locator::{LocatorError, all_named_locators, locator_scope, resolve_locator};
