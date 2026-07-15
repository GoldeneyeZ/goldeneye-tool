use goldeneye_ports::IndexMode;
use tree_sitter::Node;

use super::{
    classify::{find_descendant_kind, first_identifier, first_name_like},
    node_text,
};
use crate::language_specs::language_spec;

pub(super) fn is_call(mode: IndexMode, language: &str, kind: &str) -> bool {
    let known = match language {
        "python" => kind == "call",
        "rust" | "javascript" | "typescript" | "tsx" | "go" => kind == "call_expression",
        _ => false,
    };
    if known || mode == IndexMode::Fast {
        return known;
    }
    language_spec(language).map_or_else(
        || {
            matches!(
                kind,
                "call"
                    | "call_expression"
                    | "function_call"
                    | "function_call_expression"
                    | "method_invocation"
                    | "invocation_expression"
                    | "new_expression"
                    | "object_creation_expression"
                    | "application_expression"
                    | "apply_expression"
                    | "command_call"
                    | "subroutine_call"
                    | "system_tf_call"
            )
        },
        |spec| spec.call_kinds.contains(&kind),
    )
}

pub(super) fn audited_call_target<'tree>(language: &str, node: Node<'tree>) -> Option<Node<'tree>> {
    if language == "elixir" && node.kind() == "call" {
        return node.named_child(0);
    }
    if language == "cobol" && node.kind() == "call_statement" {
        return node
            .child_by_field_name("x")
            .or_else(|| node.named_child(0));
    }
    if language == "erlang" && node.kind() == "call" {
        return node
            .child_by_field_name("expr")
            .or_else(|| node.named_child(0));
    }
    if language == "dart" && node.kind() == "selector" {
        return node.prev_named_sibling();
    }
    if language == "nasm" && node.kind() == "actual_instruction" {
        return node
            .child_by_field_name("operands")
            .or_else(|| find_descendant_kind(node, &["operands"]))
            .and_then(|operands| find_descendant_kind(operands, &["word"]));
    }
    if language == "vhdl" && node.kind() == "parenthesis_group" {
        return node.prev_named_sibling();
    }
    [
        "function",
        "callee",
        "name",
        "method",
        "command",
        "command_name",
        "target",
    ]
    .into_iter()
    .find_map(|field| node.child_by_field_name(field))
    .or_else(|| first_name_like(node))
}

pub(super) fn generic_call_target(node: Node<'_>) -> Option<Node<'_>> {
    ["callee", "name", "method", "command", "target"]
        .into_iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| first_identifier(node))
}

pub(super) fn receiver_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let type_node = find_descendant_kind(receiver, &["type_identifier", "identifier"])?;
    Some(node_text(type_node, source))
}

pub(super) fn call_short_name(callee: Node<'_>, source: &[u8]) -> String {
    for field in ["field", "property", "attribute"] {
        if let Some(value) = callee.child_by_field_name(field) {
            return node_text(value, source);
        }
    }
    if matches!(
        callee.kind(),
        "identifier" | "field_identifier" | "property_identifier"
    ) {
        return node_text(callee, source);
    }
    last_identifier(&node_text(callee, source))
}

pub(super) fn call_receiver(callee: &str) -> Option<&str> {
    let separators = ["->", "::", "."];
    separators
        .into_iter()
        .filter_map(|separator| {
            callee
                .rfind(separator)
                .map(|index| (index, separator.len()))
        })
        .max_by_key(|(index, _)| *index)
        .map(|(index, _)| callee[..index].trim())
        .filter(|receiver| !receiver.is_empty())
}

pub(super) fn receiver_looks_like_type(receiver: &str) -> bool {
    receiver
        .rsplit(['.', ':', '>', '\\'])
        .find(|segment| !segment.is_empty())
        .and_then(|segment| segment.chars().next())
        .is_some_and(char::is_uppercase)
}

pub(super) fn last_identifier(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .rfind(|segment| !segment.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_detection_preserves_mode_and_language_dispatch() {
        let cases = [
            (IndexMode::Fast, "python", "call", true),
            (IndexMode::Fast, "python", "call_expression", false),
            (IndexMode::Fast, "rust", "call_expression", true),
            (IndexMode::Fast, "java", "method_invocation", false),
            (IndexMode::Full, "java", "method_invocation", true),
            (IndexMode::Full, "unknown", "application_expression", true),
            (IndexMode::Fast, "unknown", "application_expression", false),
        ];
        for (mode, language, kind, expected) in cases {
            assert_eq!(is_call(mode, language, kind), expected, "{language}:{kind}");
        }
    }

    #[test]
    fn receiver_and_identifier_helpers_preserve_separator_rules() {
        let receivers = [
            ("pkg::Type.method", Some("pkg::Type")),
            ("ptr->field::call", Some("ptr->field")),
            (" obj . method ", Some("obj")),
            ("call", None),
            (".call", None),
        ];
        for (callee, expected) in receivers {
            assert_eq!(call_receiver(callee), expected, "{callee}");
        }

        for receiver in ["pkg::Widget", "Vendor\\Service", "Vec<T>", "Éclair"] {
            assert!(receiver_looks_like_type(receiver), "{receiver}");
        }
        for receiver in ["module.widget", "snake::lower", ""] {
            assert!(!receiver_looks_like_type(receiver), "{receiver}");
        }

        let identifiers = [
            ("foo::bar(baz)", "baz"),
            ("$receiver->method", "method"),
            ("naïve.Type", "Type"),
            ("---", ""),
        ];
        for (value, expected) in identifiers {
            assert_eq!(last_identifier(value), expected, "{value}");
        }
    }
}
