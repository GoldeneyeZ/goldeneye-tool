use serde::Serialize;
use serde_json::{Value, json};

const PAGE_SIZE: usize = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

impl ToolDefinition {
    #[must_use]
    pub fn new(name: &str, title: &str, description: &str, input_schema: Value) -> Self {
        Self {
            name: name.to_owned(),
            title: title.to_owned(),
            description: description.to_owned(),
            input_schema,
            output_schema: json!({"type": "object", "additionalProperties": true}),
        }
    }

    #[must_use]
    pub fn with_output_schema(mut self, output_schema: Value) -> Self {
        self.output_schema = output_schema;
        self
    }

    #[cfg(test)]
    #[must_use]
    pub fn test(name: &str) -> Self {
        Self::new(name, name, name, json!({"type": "object"}))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPage {
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
}

impl ToolRegistry {
    #[must_use]
    pub const fn new(tools: Vec<ToolDefinition>) -> Self {
        Self { tools }
    }

    #[must_use]
    pub fn implemented() -> Self {
        Self::new(implemented_tools())
    }

    /// Returns one page of tool definitions beginning at `cursor`.
    ///
    /// # Errors
    ///
    /// Returns `"invalid cursor"` when `cursor` is not a valid offset into
    /// this registry.
    pub fn page(&self, cursor: Option<&str>) -> Result<ToolPage, &'static str> {
        let Some(cursor) = cursor else {
            return Ok(ToolPage {
                tools: self.tools.clone(),
                next_cursor: None,
            });
        };
        let offset = cursor.parse::<usize>().map_err(|_| "invalid cursor")?;
        if offset > self.tools.len() {
            return Err("invalid cursor");
        }
        let end = (offset + PAGE_SIZE).min(self.tools.len());
        Ok(ToolPage {
            tools: self.tools[offset..end].to_vec(),
            next_cursor: (end < self.tools.len()).then(|| end.to_string()),
        })
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::implemented()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    content: Vec<TextContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_content: Option<Value>,
    is_error: bool,
}

impl ToolCallResult {
    #[must_use]
    pub fn success(value: Value) -> Self {
        Self {
            content: vec![TextContent {
                content_type: "text",
                text: value.to_string(),
            }],
            structured_content: Some(value),
            is_error: false,
        }
    }

    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent {
                content_type: "text",
                text: text.into(),
            }],
            structured_content: None,
            is_error: true,
        }
    }
}

#[derive(Debug, Serialize)]
struct TextContent {
    #[serde(rename = "type")]
    content_type: &'static str,
    text: String,
}

fn object_schema(properties: &Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn implemented_tools() -> Vec<ToolDefinition> {
    let project = json!({"type": "string", "description": "Indexed project name"});
    let project_only = object_schema(&json!({"project": project.clone()}), &["project"]);
    let trace_schema = object_schema(
        &json!({
            "project": project.clone(),
            "function_name": {"type": "string"},
            "direction": {"type": "string", "enum": ["inbound", "outbound", "both"], "default": "both"},
            "depth": {"type": "integer", "minimum": 1, "maximum": 16, "default": 1},
            "edge_types": {"type": "array", "items": {"type": "string"}},
            "mode": {"type": "string", "enum": ["calls"], "default": "calls"}
        }),
        &["project", "function_name"],
    );
    let mut tools = index_and_metadata_tools(&project_only);
    tools.extend(search_and_query_tools(&project));
    tools.extend(trace_and_source_tools(&project, &trace_schema));
    tools.extend(edit_tools(&project));
    tools
}

fn index_and_metadata_tools(project_only: &Value) -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "index_repository",
            "Index repository",
            "Index one allowed repository in fast mode and persist its graph.",
            object_schema(
                &json!({
                    "repo_path": {"type": "string", "description": "Path to the repository"},
                    "mode": {"type": "string", "enum": ["fast"], "default": "fast"}
                }),
                &["repo_path"],
            ),
        ),
        ToolDefinition::new(
            "list_projects",
            "List projects",
            "List persisted indexed projects.",
            object_schema(&json!({}), &[]),
        ),
        ToolDefinition::new(
            "index_status",
            "Index status",
            "Return persisted graph counts and generation for one project.",
            project_only.clone(),
        ),
        ToolDefinition::new(
            "get_graph_schema",
            "Get graph schema",
            "Return node labels, edge types, counts, and properties.",
            project_only.clone(),
        ),
    ]
}

fn search_and_query_tools(project: &Value) -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "search_graph",
            "Search graph",
            "Search indexed symbols by text or regular-expression filters with cursor pagination.",
            object_schema(
                &json!({
                    "project": project.clone(),
                    "query": {"type": "string"},
                    "name_pattern": {"type": "string"},
                    "qn_pattern": {"type": "string"},
                    "label": {"type": "string"},
                    "file_pattern": {"type": "string"},
                    "relationship": {"type": "string"},
                    "min_degree": {"type": "integer", "minimum": 0},
                    "max_degree": {"type": "integer", "minimum": 0},
                    "exclude_entry_points": {"type": "boolean", "default": false},
                    "include_connected": {"type": "boolean", "default": false},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 20},
                    "offset": {"type": "integer", "minimum": 0, "default": 0},
                    "cursor": {"type": "string"}
                }),
                &["project"],
            ),
        ),
        ToolDefinition::new(
            "query_graph",
            "Query graph",
            "Execute the supported read-only Cypher subset with a bounded row count.",
            object_schema(
                &json!({
                    "project": project.clone(),
                    "query": {"type": "string"},
                    "max_rows": {"type": "integer", "minimum": 1, "maximum": 100_000, "default": 200}
                }),
                &["project", "query"],
            ),
        ),
    ]
}

fn trace_and_source_tools(project: &Value, trace_schema: &Value) -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "trace_path",
            "Trace path",
            "Trace CALLS relationships inbound, outbound, or both.",
            trace_schema.clone(),
        ),
        ToolDefinition::new(
            "trace_call_path",
            "Trace call path",
            "Compatibility alias for trace_path.",
            trace_schema.clone(),
        ),
        ToolDefinition::new(
            "get_code_snippet",
            "Get code snippet",
            "Resolve an exact, suffix, or unique short symbol name and return bounded source.",
            object_schema(
                &json!({
                    "project": project.clone(),
                    "qualified_name": {"type": "string", "description": "Exact or short symbol name"}
                }),
                &["project", "qualified_name"],
            ),
        ),
        ToolDefinition::new(
            "get_architecture",
            "Get architecture",
            "Return compact project counts, languages, modules, types, and entry points.",
            object_schema(
                &json!({
                    "project": project.clone(),
                    "aspects": {"type": "array", "items": {"type": "string"}}
                }),
                &["project"],
            ),
        ),
    ]
}

fn edit_tools(project: &Value) -> Vec<ToolDefinition> {
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
    let inspection_output = object_schema(
        &json!({
            "project": project.clone(),
            "path": path.clone(),
            "language_id": {"type": "string"},
            "file_hash": content_hash_schema(),
            "generation": {"type": "integer", "minimum": 0},
            "syntax": {"type": "object"},
            "locators": {"type": "array", "items": locator.clone()},
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
    );
    let mutation_output = object_schema(
        &json!({
            "operation_id": {"type": "string"},
            "project": project.clone(),
            "path": path.clone(),
            "old_file_hash": {"anyOf": [content_hash_schema(), {"type": "null"}]},
            "new_file_hash": content_hash_schema(),
            "diff": {"type": "object"},
            "changed_syntax_ids": {"type": "array", "items": locator.clone()},
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
    );
    let inspection_request = object_schema(
        &json!({
            "project": project.clone(),
            "path": path.clone(),
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
    );
    let content_request = object_schema(
        &json!({
            "operation_id": operation_id.clone(),
            "locator": locator.clone(),
            "content": {"type": "string"},
            "parse_policy": parse_policy.clone()
        }),
        &["operation_id", "locator", "content"],
    );
    vec![
        ToolDefinition::new(
            "inspect_syntax",
            "Inspect syntax",
            "Return compact named-node syntax and guarded full locators for one indexed file.",
            inspection_request,
        )
        .with_output_schema(inspection_output),
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
                    "parse_policy": parse_policy.clone(),
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
        .with_output_schema(mutation_output.clone()),
        ToolDefinition::new(
            "replace_node",
            "Replace node",
            "Replace exactly one locator-identified named node; stale locators never write.",
            content_request.clone(),
        )
        .with_output_schema(mutation_output.clone()),
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
        .with_output_schema(mutation_output.clone()),
        ToolDefinition::new(
            "insert_before_node",
            "Insert before node",
            "Insert content immediately before one locator-identified named node.",
            content_request.clone(),
        )
        .with_output_schema(mutation_output.clone()),
        ToolDefinition::new(
            "insert_after_node",
            "Insert after node",
            "Insert content immediately after one locator-identified named node.",
            content_request,
        )
        .with_output_schema(mutation_output),
    ]
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

#[cfg(test)]
mod tests {
    use super::{ToolDefinition, ToolRegistry};
    use serde_json::json;

    #[test]
    fn registry_returns_all_without_cursor_and_pages_when_cursor_present() {
        let tools = (0..10)
            .map(|index| ToolDefinition::test(&format!("tool-{index}")))
            .collect();
        let registry = ToolRegistry::new(tools);

        let all = registry.page(None).expect("unpaginated list");
        assert_eq!(all.tools.len(), 10);
        assert!(all.next_cursor.is_none());

        let first = registry.page(Some("0")).expect("first page");
        assert_eq!(first.tools.len(), 8);
        assert_eq!(first.next_cursor.as_deref(), Some("8"));

        let second = registry
            .page(first.next_cursor.as_deref())
            .expect("second page");
        assert_eq!(second.tools.len(), 2);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn explicit_empty_registry_advertises_no_tools() {
        let page = ToolRegistry::new(Vec::new())
            .page(None)
            .expect("empty page");

        assert!(page.tools.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn invalid_or_out_of_range_cursor_is_rejected() {
        let registry = ToolRegistry::new(vec![ToolDefinition::test("only")]);

        assert_eq!(
            registry.page(Some("not-a-number")).unwrap_err(),
            "invalid cursor"
        );
        assert_eq!(registry.page(Some("2")).unwrap_err(), "invalid cursor");
    }

    #[test]
    fn tool_definition_serializes_upstream_schema_fields() {
        let registry = ToolRegistry::new(vec![ToolDefinition::test("implemented")]);
        let page = registry.page(None).expect("first page");
        let value = serde_json::to_value(page).expect("serialize page");

        assert_eq!(
            value,
            json!({
                "tools": [{
                    "name": "implemented",
                    "title": "implemented",
                    "description": "implemented",
                    "inputSchema": {"type": "object"},
                    "outputSchema": {"type": "object", "additionalProperties": true}
                }]
            })
        );
    }
}
