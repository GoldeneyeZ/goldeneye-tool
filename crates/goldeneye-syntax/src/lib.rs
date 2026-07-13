//! Tree-sitter syntax services for Goldeneye.

mod engine;
mod grammar;
mod inspect;
mod locator;
mod pack;

pub use engine::{
    DiagnosticKind, MAX_DIAGNOSTIC_DETAILS, ReparseResult, SyntaxDiagnostic, SyntaxEdit,
    SyntaxEngine, SyntaxSnapshot,
};
pub use grammar::{
    CoreGrammarProvider, EditContentRegion, EditPointKind, Grammar, GrammarProvider, GrammarSource,
    SyntaxError,
};
pub use inspect::{
    DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES, DEFAULT_PREVIEW_CHARS, InspectError, InspectRequest,
    MAX_INSPECT_DEPTH, MAX_INSPECT_NODES, MAX_PREVIEW_CHARS, SyntaxInspection, SyntaxNodeView,
    inspect_syntax,
};
pub use locator::{LocatorError, all_named_locators, locator_scope, resolve_locator};
pub use pack::{
    GrammarPackLock, GrammarRecord, LanguageBindingStatus, LanguageMapping, PackError,
    VerifiedPack, hash_grammar_assets, lock_file_hash,
};
