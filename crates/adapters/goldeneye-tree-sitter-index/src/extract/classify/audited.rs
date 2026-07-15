use super::{
    Definition, LanguageSpec, Node, Scope, ScopeKind, ancestor_kind, find_descendant_kind,
    first_name_like, first_quoted_value, node_text,
};

pub(super) fn classify_audited(
    spec: &LanguageSpec,
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    let kind = node.kind();

    if let Some(name) = audited_special_import_name(language, node, source) {
        return Some(Definition {
            label: "Import",
            name,
        });
    }

    // In Elixir, definitions, imports, branches, assignments, and ordinary calls
    // all share the grammar's `call`/`binary_operator` nodes. The upstream
    // extractor distinguishes the defining macro before applying its tables.
    if language == "elixir" && kind == "call" {
        return classify_elixir_call(node, source);
    }
    if matches!(language, "k8s" | "kustomize") && kind == "block_mapping_pair" {
        return classify_kubernetes_resource(node, source);
    }
    if language == "meson" && kind == "operatorunit" {
        let text = node_text(node, source);
        if text.contains("= func") {
            return classify_meson_function(node, source);
        }
    }
    let label = audited_label(language, node, scope, source, spec)?;
    audited_definition_name(language, node, label, source).map(|name| Definition { label, name })
}

fn classify_meson_function(node: Node<'_>, source: &[u8]) -> Option<Definition> {
    let name_node = node.named_child(0).and_then(first_name_like)?;
    let name = node_text(name_node, source);
    (!name.is_empty()).then_some(Definition {
        label: "Function",
        name,
    })
}

fn audited_label(
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
    spec: &LanguageSpec,
) -> Option<&'static str> {
    let kind = node.kind();
    Some(if spec.function_kinds.contains(&kind) {
        if matches!(language, "capnp" | "protobuf" | "smali") {
            "Function"
        } else if scope.kind == ScopeKind::Type {
            "Method"
        } else {
            "Function"
        }
    } else if spec.class_kinds.contains(&kind) {
        audited_class_label(language, node, source)
    } else if spec.field_kinds.contains(&kind) {
        "Field"
    } else if spec.module_kinds.contains(&kind) && node.parent().is_some() {
        "Module"
    } else if spec.import_from_kinds.contains(&kind)
        || (spec.import_kinds.contains(&kind)
            && !spec.call_kinds.contains(&kind)
            && !spec.branch_kinds.contains(&kind)
            && !spec.throw_kinds.contains(&kind)
            && !spec.decorator_kinds.contains(&kind))
    {
        "Import"
    } else if spec.variable_kinds.contains(&kind) || spec.assignment_kinds.contains(&kind) {
        if scope.kind == ScopeKind::Type {
            "Field"
        } else {
            "Variable"
        }
    } else {
        return None;
    })
}

fn audited_special_import_name(language: &str, node: Node<'_>, source: &[u8]) -> Option<String> {
    let kind = node.kind();
    let text = node_text(node, source);
    let keyword = match language {
        "crystal" if kind == "require" => "require",
        "dart" if matches!(kind, "import_or_export" | "import") => "import",
        "kotlin" if matches!(kind, "import" | "import_header") => "import",
        "puppet" if matches!(kind, "include_statement" | "include") => "include",
        "puppet" if matches!(kind, "require_statement" | "require") => "require",
        "r" if kind == "call" => [
            "library",
            "require",
            "requireNamespace",
            "loadNamespace",
            "source",
        ]
        .into_iter()
        .find(|keyword| starts_with_call(&text, keyword))?,
        "ruby" if matches!(kind, "call" | "command_call") => ["require_relative", "require"]
            .into_iter()
            .find(|keyword| starts_with_call(&text, keyword))?,
        _ => return None,
    };
    import_name_after_keyword(&text, keyword)
}

fn starts_with_call(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix(keyword) else {
        return false;
    };
    rest.starts_with(|character: char| character.is_whitespace() || character == '(')
}

pub(super) fn import_name_after_keyword(text: &str, keyword: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix(keyword)?.trim_start();
    if let Some(value) = first_quoted_value(rest) {
        return Some(value);
    }
    let name = rest
        .trim_start_matches('(')
        .trim_start()
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, ')' | ';' | ',' | '{' | '[' | '\n' | '\r')
        })
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| matches!(character, '"' | '\'' | '(' | ')'));
    (!name.is_empty()).then(|| name.to_owned())
}

fn classify_kubernetes_resource(node: Node<'_>, source: &[u8]) -> Option<Definition> {
    let key = node
        .child_by_field_name("key")
        .or_else(|| node.named_child(0))?;
    if node_text(key, source).trim() != "kind" {
        return None;
    }
    let value = node
        .child_by_field_name("value")
        .or_else(|| node.named_child(1))?;
    let name = node_text(value, source)
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'')
        })
        .to_owned();
    (!name.is_empty()).then_some(Definition {
        label: "Resource",
        name,
    })
}

fn audited_class_label(language: &str, node: Node<'_>, source: &[u8]) -> &'static str {
    let kind = node.kind();
    if language == "markdown" && matches!(kind, "atx_heading" | "setext_heading") {
        return "Section";
    }
    if matches!(language, "sway" | "wgsl") && matches!(kind, "struct_item" | "struct_declaration") {
        return "Struct";
    }
    if language == "sway" && kind == "abi_item" {
        return "Interface";
    }
    if matches!(language, "swift" | "dlang") && matches!(kind, "struct_item" | "struct_declaration")
    {
        return "Struct";
    }
    if language == "swift" && kind == "class_declaration" {
        let declaration_kind = node
            .child_by_field_name("declaration_kind")
            .map(|value| node_text(value, source));
        if declaration_kind.as_deref() == Some("struct") {
            return "Struct";
        }
    }
    if matches!(
        kind,
        "interface_declaration"
            | "interface_type"
            | "trait_item"
            | "trait_definition"
            | "protocol_declaration"
    ) {
        "Interface"
    } else if matches!(kind, "enum_specifier" | "enum_declaration" | "enum_item") {
        "Enum"
    } else if matches!(
        kind,
        "type_alias_declaration" | "type_item" | "type_alias" | "type_definition"
    ) {
        "Type"
    } else {
        "Class"
    }
}

fn classify_elixir_call(node: Node<'_>, source: &[u8]) -> Option<Definition> {
    let callee = node.child(0)?;
    let macro_name = node_text(callee, source);
    let label = match macro_name.as_str() {
        "def" | "defp" | "defmacro" | "defmacrop" => "Function",
        "defmodule" | "defprotocol" | "defimpl" => "Class",
        _ => return None,
    };
    let argument = node
        .child_by_field_name("arguments")
        .or_else(|| node.named_child(1))?;
    let name_node = if label == "Function" {
        find_descendant_kind(argument, &["identifier"])
    } else {
        find_descendant_kind(argument, &["alias", "identifier"])
    }?;
    let name = node_text(name_node, source).trim().to_owned();
    (!name.is_empty()).then_some(Definition { label, name })
}

fn audited_definition_name(
    language: &str,
    node: Node<'_>,
    label: &str,
    source: &[u8],
) -> Option<String> {
    let name_node = audited_name_node(language, node, label)?;
    let raw_name = if matches!(label, "Variable" | "Field") {
        first_name_like(name_node).map_or_else(
            || node_text(name_node, source),
            |value| node_text(value, source),
        )
    } else {
        node_text(name_node, source)
    };
    let name = raw_name
        .trim_matches(|character: char| {
            matches!(character, '"' | '\'' | '`' | '<' | '>' | ':' | ';')
        })
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[allow(clippy::too_many_lines)]
fn audited_name_node<'tree>(language: &str, node: Node<'tree>, label: &str) -> Option<Node<'tree>> {
    let kind = node.kind();
    if language == "zig" && kind == "test_declaration" {
        return find_descendant_kind(node, &["string_content"]);
    }
    if language == "zig"
        && matches!(
            kind,
            "struct_declaration" | "enum_declaration" | "union_declaration"
        )
        && let Some(parent) = node.parent()
        && parent.kind() == "variable_declaration"
    {
        return find_descendant_kind(parent, &["identifier"]);
    }
    if language == "lean"
        && let Some(decl_id) = node.child_by_field_name("declId")
    {
        return first_name_like(decl_id).or(Some(decl_id));
    }
    if language == "haskell" && matches!(label, "Function" | "Method") {
        return find_descendant_kind(node, &["variable", "name", "identifier"]);
    }
    if language == "commonlisp" && matches!(label, "Function" | "Method") {
        return find_descendant_kind(node, &["function_name", "sym_lit", "symbol"]);
    }
    if language == "makefile" && kind == "rule" {
        return find_descendant_kind(node, &["word"]);
    }
    if language == "meson" && kind == "function_expression" {
        return ancestor_kind(node, "assignment_statement", 3)
            .or_else(|| ancestor_kind(node, "assignment", 3))
            .and_then(|assignment| {
                assignment
                    .child_by_field_name("left")
                    .or_else(|| assignment.named_child(0))
            })
            .and_then(|left| first_name_like(left).or(Some(left)));
    }
    if language == "elm" && kind == "value_declaration" {
        return node
            .child_by_field_name("functionDeclarationLeft")
            .or_else(|| find_descendant_kind(node, &["function_declaration_left"]))
            .and_then(|left| find_descendant_kind(left, &["lower_case_identifier"]));
    }
    if language == "ocaml" && kind == "value_definition" {
        return find_descendant_kind(node, &["let_binding"])
            .and_then(|binding| binding.child_by_field_name("pattern"));
    }
    if language == "rescript" && kind == "function" {
        return ancestor_kind(node, "let_binding", 3)
            .and_then(|binding| binding.child_by_field_name("pattern"));
    }
    if language == "nickel" && kind == "fun_expr" {
        return ancestor_kind(node, "let_binding", 8)
            .and_then(|binding| binding.child_by_field_name("pat"))
            .and_then(|pattern| pattern.child_by_field_name("pat").or(Some(pattern)));
    }
    if language == "nix" && kind == "function_expression" {
        return ancestor_kind(node, "binding", 2)
            .and_then(|binding| binding.child_by_field_name("attrpath"))
            .and_then(|path| path.child_by_field_name("attr").or(Some(path)));
    }
    if language == "r" && kind == "function_definition" {
        return node.parent().and_then(|parent| {
            (parent.kind() == "binary_operator")
                .then(|| {
                    parent
                        .child_by_field_name("lhs")
                        .or_else(|| parent.named_child(0))
                })
                .flatten()
        });
    }
    if language == "lua" && kind == "function_definition" {
        let parent = node.parent().and_then(|parent| {
            (parent.kind() == "expression_list")
                .then(|| parent.parent())
                .flatten()
                .or(Some(parent))
        });
        if let Some(parent) = parent.filter(|parent| parent.kind() == "assignment_statement") {
            return parent
                .child_by_field_name("variables")
                .or_else(|| find_descendant_kind(parent, &["variable_list"]))
                .and_then(|variables| variables.named_child(0).or_else(|| variables.child(0)));
        }
    }
    if language == "fortran" && matches!(kind, "subroutine" | "function") {
        return find_descendant_kind(node, &["subroutine_statement", "function_statement"])
            .and_then(|statement| statement.child_by_field_name("name"));
    }
    if language == "typst" && kind == "let" {
        return node.child_by_field_name("pattern").and_then(|pattern| {
            (pattern.kind() == "call")
                .then(|| pattern.child_by_field_name("item"))
                .flatten()
        });
    }
    if language == "wolfram"
        && matches!(kind, "set_delayed_top" | "set_top" | "set_delayed" | "set")
    {
        return node.named_child(0).and_then(|left| {
            if left.kind() == "apply" {
                find_descendant_kind(left, &["user_symbol", "builtin_symbol"])
            } else if matches!(left.kind(), "user_symbol" | "builtin_symbol") {
                Some(left)
            } else {
                None
            }
        });
    }
    if language == "markdown" && matches!(kind, "atx_heading" | "setext_heading") {
        return Some(node);
    }

    let fields: &[&str] = match label {
        "Import" => &[
            "source",
            "path",
            "module_name",
            "module",
            "name",
            "argument",
        ],
        "Variable" | "Field" => &[
            "left",
            "name",
            "pattern",
            "declarator",
            "target",
            "property",
        ],
        "Module" => &["name", "module", "module_name"],
        _ => &["name", "declarator", "identifier", "type", "target"],
    };
    fields
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .and_then(|candidate| first_name_like(candidate).or(Some(candidate)))
        .or_else(|| first_name_like(node))
}
