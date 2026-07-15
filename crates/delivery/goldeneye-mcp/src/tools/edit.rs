use serde_json::{Value, json};

use super::{ToolDefinition, object_schema};

struct EditPrimitives {
    path: Value,
    operation_id: Value,
    parse_policy: Value,
    locator: Value,
}

struct EditSchemas {
    inspection_output: Value,
    mutation_output: Value,
    inspection_request: Value,
    content_request: Value,
}

pub(super) fn tools(project: &Value) -> Vec<ToolDefinition> {
    let primitives = edit_primitives();
    let schemas = edit_schemas(project, &primitives);
    let EditPrimitives {
        path,
        operation_id,
        parse_policy,
        locator,
    } = primitives;
    let EditSchemas {
        inspection_output,
        mutation_output,
        inspection_request,
        content_request,
    } = schemas;
    vec![
        inspection_tool(inspection_request, inspection_output),
        create_tool(
            project,
            operation_id,
            path,
            parse_policy.clone(),
            mutation_output.clone(),
        ),
        replace_tool(content_request.clone(), mutation_output.clone()),
        delete_tool(locator, parse_policy, mutation_output.clone()),
        insert_before_tool(content_request.clone(), mutation_output.clone()),
        insert_after_tool(content_request, mutation_output),
    ]
}

fn edit_primitives() -> EditPrimitives {
    let path = json!({
        "type": "string",
        "minLength": 1,
        "description": "Validated project-relative path"
    });
    let operation_id = json!({
        "type": "string",
        "minLength": 1,
        "description": "Unique durable journal operation ID"
    });
    let parse_policy = json!({
        "type": "string",
        "enum": ["require_clean", "no_additional_diagnostics", "allow_errors"]
    });
    let locator = locator_schema();
    EditPrimitives {
        path,
        operation_id,
        parse_policy,
        locator,
    }
}

fn edit_schemas(project: &Value, primitives: &EditPrimitives) -> EditSchemas {
    let inspection_output = inspection_output_schema(project, primitives);
    let mutation_output = mutation_output_schema(project, primitives);
    let inspection_request = inspection_request_schema(project, primitives);
    let content_request = object_schema(
        &json!({
            "operation_id": primitives.operation_id.clone(),
            "locator": primitives.locator.clone(),
            "content": {"type": "string"},
            "parse_policy": primitives.parse_policy.clone()
        }),
        &["operation_id", "locator", "content"],
    );
    EditSchemas {
        inspection_output,
        mutation_output,
        inspection_request,
        content_request,
    }
}

fn inspection_output_schema(project: &Value, primitives: &EditPrimitives) -> Value {
    object_schema(
        &json!({
            "project": project.clone(),
            "path": primitives.path.clone(),
            "language_id": {"type": "string"},
            "file_hash": content_hash_schema(),
            "generation": {"type": "integer", "minimum": 0},
            "syntax": {"type": "object"},
            "locators": {"type": "array", "items": primitives.locator.clone()},
            "diagnostic_total": {"type": "integer", "minimum": 0},
            "diagnostics_truncated": {"type": "boolean"},
            "diagnostics": {"type": "array", "items": {"type": "object"}},
            "size": {"type": "object"}
        }),
        &[
            "project",
            "path",
            "language_id",
            "file_hash",
            "generation",
            "syntax",
            "locators",
            "diagnostic_total",
            "diagnostics_truncated",
            "diagnostics",
            "size",
        ],
    )
}

fn mutation_output_schema(project: &Value, primitives: &EditPrimitives) -> Value {
    object_schema(
        &json!({
            "operation_id": {"type": "string"},
            "project": project.clone(),
            "path": primitives.path.clone(),
            "old_file_hash": {"anyOf": [content_hash_schema(), {"type": "null"}]},
            "new_file_hash": content_hash_schema(),
            "diff": {"type": "object"},
            "changed_syntax_ids": {"type": "array", "items": primitives.locator.clone()},
            "changed_graph_ids": {"type": "array", "items": {"type": "string"}},
            "graph": {"type": "object"},
            "generation": {"type": "integer", "minimum": 0},
            "diagnostics": {"type": "object"},
            "size": {"type": "object"}
        }),
        &[
            "operation_id",
            "project",
            "path",
            "old_file_hash",
            "new_file_hash",
            "diff",
            "changed_syntax_ids",
            "changed_graph_ids",
            "graph",
            "generation",
            "diagnostics",
            "size",
        ],
    )
}

fn inspection_request_schema(project: &Value, primitives: &EditPrimitives) -> Value {
    object_schema(
        &json!({
            "project": project.clone(),
            "path": primitives.path.clone(),
            "inspect": object_schema(
                &json!({
                    "max_depth": {"type": "integer", "minimum": 0, "maximum": 32, "default": 4},
                    "max_nodes": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 200},
                    "preview_chars": {"type": "integer", "minimum": 0, "maximum": 256, "default": 0},
                    "byte_range": object_schema(
                        &json!({
                            "start": {"type": "integer", "minimum": 0},
                            "end": {"type": "integer", "minimum": 0}
                        }),
                        &["start", "end"],
                    ),
                    "node_kinds": {
                        "type": "array",
                        "maxItems": 32,
                        "items": {"type": "string", "minLength": 1}
                    }
                }),
                &[],
            )
        }),
        &["project", "path"],
    )
}

fn inspection_tool(input: Value, output: Value) -> ToolDefinition {
    ToolDefinition::new(
        "inspect_syntax",
        "Inspect syntax",
        "Return compact named-node syntax and guarded full locators for one indexed file.",
        input,
    )
    .with_output_schema(output)
}

// Ownership mirrors the original single builder: these schema values are retired after this tool.
#[allow(clippy::needless_pass_by_value)]
fn create_tool(
    project: &Value,
    operation_id: Value,
    path: Value,
    parse_policy: Value,
    output: Value,
) -> ToolDefinition {
    ToolDefinition::new(
        "create_file",
        "Create file",
        "Create one authorized project-relative file without overwriting an existing target.",
        object_schema(
            &json!({
                "operation_id": operation_id,
                "project": project.clone(),
                "path": path,
                "content": {"type": "string"},
                "expected_generation": {"type": "integer", "minimum": 0},
                "language_id": {"type": "string", "minLength": 1},
                "parse_policy": parse_policy,
                "create_parents": {"type": "boolean", "default": false}
            }),
            &[
                "operation_id",
                "project",
                "path",
                "content",
                "expected_generation",
            ],
        ),
    )
    .with_output_schema(output)
}

fn replace_tool(input: Value, output: Value) -> ToolDefinition {
    ToolDefinition::new(
        "replace_node",
        "Replace node",
        "Replace exactly one locator-identified named node; stale locators never write.",
        input,
    )
    .with_output_schema(output)
}

#[allow(clippy::needless_pass_by_value)]
fn delete_tool(locator: Value, parse_policy: Value, output: Value) -> ToolDefinition {
    ToolDefinition::new(
        "delete_node",
        "Delete node",
        "Delete exactly one locator-identified named node; stale locators never write.",
        object_schema(
            &json!({
                "operation_id": {"type": "string", "minLength": 1},
                "locator": locator,
                "parse_policy": parse_policy
            }),
            &["operation_id", "locator"],
        ),
    )
    .with_output_schema(output)
}

fn insert_before_tool(input: Value, output: Value) -> ToolDefinition {
    ToolDefinition::new(
        "insert_before_node",
        "Insert before node",
        "Insert content immediately before one locator-identified named node.",
        input,
    )
    .with_output_schema(output)
}

fn insert_after_tool(input: Value, output: Value) -> ToolDefinition {
    ToolDefinition::new(
        "insert_after_node",
        "Insert after node",
        "Insert content immediately after one locator-identified named node.",
        input,
    )
    .with_output_schema(output)
}

fn content_hash_schema() -> Value {
    json!({"type": "string", "pattern": "^[0-9a-f]{64}$"})
}

fn locator_schema() -> Value {
    let byte_span = object_schema(
        &json!({
            "start": {"type": "integer", "minimum": 0},
            "end": {"type": "integer", "minimum": 0}
        }),
        &["start", "end"],
    );
    let point = object_schema(
        &json!({
            "row": {"type": "integer", "minimum": 0},
            "column_bytes": {"type": "integer", "minimum": 0}
        }),
        &["row", "column_bytes"],
    );
    let source_span = object_schema(
        &json!({"bytes": byte_span, "start": point.clone(), "end": point}),
        &["bytes", "start", "end"],
    );
    let ancestor = object_schema(
        &json!({
            "node_kind": {"type": "string", "minLength": 1},
            "named_child_index": {"type": "integer", "minimum": 0},
            "field_name": {"anyOf": [{"type": "string", "minLength": 1}, {"type": "null"}]}
        }),
        &["node_kind", "named_child_index", "field_name"],
    );
    object_schema(
        &json!({
            "scope": object_schema(
                &json!({
                    "file": object_schema(
                        &json!({
                            "project_id": {"type": "string", "minLength": 1},
                            "relative_path": {"type": "string", "minLength": 1}
                        }),
                        &["project_id", "relative_path"],
                    ),
                    "language_id": {"type": "string", "minLength": 1},
                    "grammar": object_schema(
                        &json!({
                            "provider": {"type": "string", "minLength": 1},
                            "grammar": {"type": "string", "minLength": 1},
                            "revision": {"type": "string", "minLength": 1},
                            "abi": {"type": "integer", "minimum": 0}
                        }),
                        &["provider", "grammar", "revision", "abi"],
                    ),
                    "file_hash": content_hash_schema(),
                    "generation": {"type": "integer", "minimum": 0}
                }),
                &["file", "language_id", "grammar", "file_hash", "generation"],
            ),
            "anchor": object_schema(
                &json!({
                    "ancestor_path": {"type": "array", "items": ancestor},
                    "node_kind": {"type": "string", "minLength": 1},
                    "source_span": source_span,
                    "content_hash": content_hash_schema()
                }),
                &["ancestor_path", "node_kind", "source_span", "content_hash"],
            )
        }),
        &["scope", "anchor"],
    )
}
