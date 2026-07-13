//! Tree-sitter syntax services for Goldeneye.

mod engine;
mod grammar;
mod locator;

pub use engine::{
    DiagnosticKind, MAX_DIAGNOSTIC_DETAILS, ReparseResult, SyntaxDiagnostic, SyntaxEdit,
    SyntaxEngine, SyntaxSnapshot,
};
pub use grammar::{
    CoreGrammarProvider, EditContentRegion, EditPointKind, Grammar, GrammarProvider, GrammarSource,
    SyntaxError,
};
pub use locator::{LocatorError, all_named_locators, locator_scope, resolve_locator};
