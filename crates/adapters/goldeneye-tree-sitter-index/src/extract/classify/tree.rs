use super::{Node, import_name_after_keyword};

pub(in super::super) fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    find_descendant_kind(
        node,
        &[
            "identifier",
            "field_identifier",
            "property_identifier",
            "type_identifier",
        ],
    )
}

pub(in super::super) fn first_name_like(node: Node<'_>) -> Option<Node<'_>> {
    find_descendant_kind(
        node,
        &[
            "identifier",
            "ident",
            "id",
            "field_identifier",
            "property_identifier",
            "type_identifier",
            "namespace_identifier",
            "simple_identifier",
            "simple_name",
            "constant",
            "constant_identifier",
            "variable",
            "variable_name",
            "name",
            "alias",
            "symbol",
            "sym_lit",
            "function_name",
            "rpc_name",
            "method_identifier",
            "enum_identifier",
            "program_name",
            "lower_case_identifier",
            "upper_case_identifier",
            "long_identifier",
            "operator_identifier",
            "user_symbol",
            "builtin_symbol",
            "word",
            "key",
            "bare_key",
            "dotted_key",
            "section_name",
            "tag_name",
            "Name",
            "module_path",
            "qid",
            "atom",
            "command_name",
            "service_name",
            "message_name",
            "enum_name",
            "data_name",
            "record_name",
            "class_identifier",
            "string_content",
        ],
    )
}

pub(super) fn ancestor_kind<'tree>(
    node: Node<'tree>,
    kind: &str,
    max_depth: usize,
) -> Option<Node<'tree>> {
    let mut current = node.parent();
    for _ in 0..max_depth {
        let ancestor = current?;
        if ancestor.kind() == kind {
            return Some(ancestor);
        }
        current = ancestor.parent();
    }
    None
}

pub(in super::super) fn find_descendant_kind<'tree>(
    node: Node<'tree>,
    kinds: &[&str],
) -> Option<Node<'tree>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_descendant_kind(child, kinds))
}
pub(in super::super) fn gomod_requirement_name(text: &str) -> Option<String> {
    import_name_after_keyword(text, "require")
}
