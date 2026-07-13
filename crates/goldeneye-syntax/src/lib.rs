//! Tree-sitter syntax services for Goldeneye.

mod engine;
mod grammar;

pub use engine::{
    DiagnosticKind, MAX_DIAGNOSTIC_DETAILS, ReparseResult, SyntaxDiagnostic, SyntaxEdit,
    SyntaxEngine, SyntaxSnapshot,
};
pub use grammar::{
    CoreGrammarProvider, EditContentRegion, EditPointKind, Grammar, GrammarProvider, GrammarSource,
    SyntaxError,
};
