use goldeneye_domain::NodeId;
use goldeneye_ports::IndexMode;
use tree_sitter::Node;

use super::{first_quoted_value, node_text};
use crate::language_specs::{LanguageSpec, language_spec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScopeKind {
    Module,
    Type,
    Callable,
}

#[derive(Debug, Clone)]
pub(super) struct Scope {
    pub(super) parent: NodeId,
    pub(super) qualified_name: String,
    pub(super) kind: ScopeKind,
    pub(super) callable: Option<NodeId>,
}

pub(super) struct Definition {
    pub(super) label: &'static str,
    pub(super) name: String,
}

mod audited;
mod generic;
mod tree;

use audited::{classify_audited, import_name_after_keyword};
use generic::{classify_generic, classify_known};
use tree::ancestor_kind;
pub(super) use tree::{
    find_descendant_kind, first_identifier, first_name_like, gomod_requirement_name,
};

pub(super) fn classify(
    mode: IndexMode,
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    if language == "graphql" && node.kind() == "type_definition" {
        return None;
    }
    if let Some(definition) = classify_known(language, node, scope, source) {
        return Some(definition);
    }
    if mode == IndexMode::Fast {
        return None;
    }
    language_spec(language).map_or_else(
        || classify_generic(node, scope, source),
        |spec| classify_audited(spec, language, node, scope, source),
    )
}
