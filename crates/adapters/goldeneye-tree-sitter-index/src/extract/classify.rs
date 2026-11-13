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

use audited::{
    classify_audited, declares_javascript_function, import_name_after_keyword, is_javascript_family,
};
use generic::{classify_generic, classify_known};
use tree::ancestor_kind;
pub(super) use tree::{
    find_descendant_kind, first_identifier, first_name_like, gomod_requirement_name,
};

/// Nodes that would otherwise become a second symbol, or a symbol named after
/// whatever identifier comes first inside them.
fn declares_nothing_of_its_own(language: &str, node: Node<'_>) -> bool {
    let kind = node.kind();
    // Makefile: the rule is the symbol, named by its target. The commands
    // under it were a second function, named after the first variable they use.
    if language == "makefile" && kind == "recipe" {
        return true;
    }
    // Go: `type X struct { .. }` is a declaration around a specification. The
    // specification is the symbol (struct, interface or type); the declaration
    // was a class of the same name on the same line, and in a grouped
    // `type ( .. )` a class named after the first of the group.
    // The same holds for `var` and `const`: each name of a declaration, also
    // of a grouped `const ( .. )`, is a symbol through its own specification.
    if language == "go"
        && matches!(
            kind,
            "type_declaration" | "var_declaration" | "const_declaration"
        )
    {
        return true;
    }
    // PowerShell: `$name` wherever it is read was a variable of its own. The
    // assignment is the symbol.
    if language == "powershell" && kind == "variable" {
        return true;
    }
    // C family: `struct stat st;` uses a type and `struct node;` announces
    // one. Each was a class; the definition is where the body is.
    if matches!(
        kind,
        "struct_specifier" | "union_specifier" | "class_specifier" | "enum_specifier"
    ) && node.child_by_field_name("body").is_none()
    {
        return true;
    }
    let unnamed = node.child_by_field_name("name").is_none();
    // C family: a struct or union without a tag was named after its first
    // field, or after that field's type. `typedef struct { .. } name_t;` is
    // the symbol `name_t` through its type definition. An enum without a tag
    // keeps the name of its first enumerator: `enum { LIMIT = 8 };` is how C
    // code writes a constant, and that name is how it is looked up.
    if unnamed
        && matches!(
            kind,
            "struct_specifier" | "union_specifier" | "class_specifier"
        )
    {
        return true;
    }
    // JavaScript: a class expression without a name was named after its first member.
    unnamed && is_javascript_family(language) && kind == "class"
}

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
    if is_javascript_family(language) && declares_javascript_function(node) {
        // The function is the symbol; it takes the declared name.
        return None;
    }
    if declares_nothing_of_its_own(language, node) {
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
