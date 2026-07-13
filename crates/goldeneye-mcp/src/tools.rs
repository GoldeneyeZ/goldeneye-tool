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
