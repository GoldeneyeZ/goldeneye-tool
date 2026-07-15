use super::{Definition, Node, Scope, ScopeKind, first_identifier, node_text};

pub(super) fn classify_known(
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    let kind = node.kind();
    let (label, field) = known_label_and_field(language, node, scope)?;
    let name_node = node
        .child_by_field_name(field)
        .or_else(|| first_identifier(node));
    let raw_name = name_node.map_or_else(
        || node_text(node, source),
        |value| {
            if matches!(
                kind,
                "short_var_declaration" | "assignment" | "let_declaration"
            ) {
                first_identifier(value).map_or_else(
                    || node_text(value, source),
                    |identifier| node_text(identifier, source),
                )
            } else {
                node_text(value, source)
            }
        },
    );
    let name = raw_name
        .trim_matches(|character: char| character == '"' || character == '\'')
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(Definition {
            label,
            name: name.to_owned(),
        })
    }
}

fn known_label_and_field(
    language: &str,
    node: Node<'_>,
    scope: &Scope,
) -> Option<(&'static str, &'static str)> {
    let kind = node.kind();
    Some(match language {
        "rust" => match kind {
            "struct_item" => ("Struct", "name"),
            "enum_item" => ("Enum", "name"),
            "trait_item" => ("Trait", "name"),
            "type_item" => ("TypeAlias", "name"),
            "function_item" if scope.kind == ScopeKind::Type => ("Method", "name"),
            "function_item" => ("Function", "name"),
            "field_declaration" => ("Field", "name"),
            "let_declaration" => ("Variable", "pattern"),
            "use_declaration" => ("Import", "argument"),
            _ => return None,
        },
        "python" => match kind {
            "class_definition" => ("Class", "name"),
            "function_definition" if scope.kind == ScopeKind::Type => ("Method", "name"),
            "function_definition" => ("Function", "name"),
            "assignment" if scope.kind == ScopeKind::Type => ("Field", "left"),
            "assignment" => ("Variable", "left"),
            "import_statement" => ("Import", "name"),
            "import_from_statement" => ("Import", "module_name"),
            _ => return None,
        },
        "javascript" | "typescript" | "tsx" => match kind {
            "class_declaration" => ("Class", "name"),
            "interface_declaration" => ("Interface", "name"),
            "type_alias_declaration" => ("TypeAlias", "name"),
            "function_declaration" => ("Function", "name"),
            "method_definition" => ("Method", "name"),
            "field_definition" => ("Field", "property"),
            "public_field_definition" => ("Field", "name"),
            "variable_declarator" => ("Variable", "name"),
            "import_statement" => ("Import", "source"),
            _ => return None,
        },
        "go" => match kind {
            "type_spec" => {
                let label = match node.child_by_field_name("type").map(|child| child.kind()) {
                    Some("struct_type") => "Struct",
                    Some("interface_type") => "Interface",
                    _ => "Type",
                };
                (label, "name")
            }
            "function_declaration" => ("Function", "name"),
            "method_declaration" => ("Method", "name"),
            "field_declaration" => ("Field", "name"),
            "short_var_declaration" => ("Variable", "left"),
            "var_spec" => ("Variable", "name"),
            "import_spec" => ("Import", "path"),
            _ => return None,
        },
        _ => return None,
    })
}

pub(super) fn classify_generic(node: Node<'_>, scope: &Scope, source: &[u8]) -> Option<Definition> {
    let kind = node.kind();
    let label = generic_definition_label(kind, scope)?;
    let name_node = generic_name_node(node, label)?;
    let raw_name = if matches!(label, "Variable" | "Field") {
        first_identifier(name_node).map_or_else(
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
    (!name.is_empty()).then(|| Definition {
        label,
        name: name.to_owned(),
    })
}

fn generic_definition_label(kind: &str, scope: &Scope) -> Option<&'static str> {
    if is_generic_method(kind) {
        return Some("Method");
    }
    if is_generic_function(kind) {
        return Some(if scope.kind == ScopeKind::Type {
            "Method"
        } else {
            "Function"
        });
    }
    generic_type_label(kind)
        .or_else(|| is_generic_field(kind).then_some("Field"))
        .or_else(|| is_generic_variable(kind).then_some("Variable"))
        .or_else(|| is_generic_import(kind).then_some("Import"))
}

fn is_generic_method(kind: &str) -> bool {
    matches!(
        kind,
        "method"
            | "method_declaration"
            | "method_definition"
            | "method_signature"
            | "constructor"
            | "constructor_declaration"
            | "constructor_definition"
            | "destructor"
            | "destructor_declaration"
            | "secondary_constructor"
    )
}

fn is_generic_function(kind: &str) -> bool {
    matches!(
        kind,
        "function"
            | "function_declaration"
            | "function_definition"
            | "function_item"
            | "function_signature"
            | "function_signature_item"
            | "function_statement"
            | "function_clause"
            | "function_def"
            | "func_def"
            | "method_elem"
            | "subroutine"
            | "subroutine_declaration"
            | "subroutine_declaration_statement"
            | "procedure"
            | "procedure_declaration"
            | "procedure_definition"
            | "procedure_definition_item"
            | "macro_definition"
            | "macro_declaration"
            | "macro_def"
            | "rpc"
    )
}

fn generic_type_label(kind: &str) -> Option<&'static str> {
    match kind {
        "class"
        | "class_declaration"
        | "class_definition"
        | "class_statement"
        | "class_specifier"
        | "class_interface"
        | "class_implementation" => Some("Class"),
        "struct"
        | "struct_item"
        | "struct_declaration"
        | "struct_definition"
        | "struct_specifier"
        | "structure"
        | "structure_declaration" => Some("Struct"),
        "enum" | "enum_item" | "enum_declaration" | "enum_definition" | "enum_specifier"
        | "enum_statement" => Some("Enum"),
        "trait" | "trait_item" | "trait_declaration" => Some("Trait"),
        "interface"
        | "interface_declaration"
        | "interface_definition"
        | "protocol_declaration"
        | "protocol_definition" => Some("Interface"),
        "type_alias" | "type_alias_declaration" | "type_alias_definition" | "type_item" => {
            Some("TypeAlias")
        }
        "type"
        | "type_declaration"
        | "type_definition"
        | "type_spec"
        | "data_type"
        | "newtype"
        | "custom_type"
        | "contract_declaration"
        | "message"
        | "service" => Some("Type"),
        _ => None,
    }
}

fn is_generic_field(kind: &str) -> bool {
    matches!(
        kind,
        "field"
            | "field_declaration"
            | "field_definition"
            | "public_field_definition"
            | "property"
            | "property_declaration"
            | "property_definition"
            | "record_field"
            | "enum_member"
    )
}

fn is_generic_variable(kind: &str) -> bool {
    matches!(
        kind,
        "let_declaration"
            | "variable_declaration"
            | "variable_declarator"
            | "variable_assignment"
            | "short_var_declaration"
            | "var_declaration"
            | "var_spec"
            | "assignment"
            | "assignment_statement"
            | "const_declaration"
    )
}

fn is_generic_import(kind: &str) -> bool {
    matches!(
        kind,
        "import"
            | "import_declaration"
            | "import_directive"
            | "import_or_export"
            | "import_spec"
            | "import_statement"
            | "import_from_statement"
            | "include"
            | "include_directive"
            | "include_statement"
            | "preproc_include"
            | "preproc_import"
            | "require_statement"
            | "use_declaration"
            | "use_statement"
            | "using_directive"
            | "using_statement"
    )
}

fn generic_name_node<'tree>(node: Node<'tree>, label: &str) -> Option<Node<'tree>> {
    let fields: &[&str] = if label == "Import" {
        &["path", "module_name", "source", "argument", "name"]
    } else if matches!(label, "Variable" | "Field") {
        &[
            "name",
            "declarator",
            "pattern",
            "left",
            "property",
            "field",
            "key",
        ]
    } else {
        &["name", "declarator", "identifier", "type"]
    };
    fields
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| first_identifier(node))
}
