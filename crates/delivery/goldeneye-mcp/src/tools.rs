mod compatibility;
mod edit;
mod index;
mod search;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ToolResponseMode {
    #[default]
    Dual,
    Text,
}

impl ToolResponseMode {
    pub(crate) const ENVIRONMENT_VARIABLE: &str = "GOLDENEYE_MCP_RESPONSE_MODE";

    pub(crate) fn from_environment() -> Result<Self, String> {
        match std::env::var(Self::ENVIRONMENT_VARIABLE) {
            Ok(value) => Self::parse(Some(&value)),
            Err(std::env::VarError::NotPresent) => Ok(Self::Dual),
            Err(std::env::VarError::NotUnicode(_)) => Err(format!(
                "{} must contain valid Unicode",
                Self::ENVIRONMENT_VARIABLE
            )),
        }
    }

    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("dual") => Ok(Self::Dual),
            Some("text") => Ok(Self::Text),
            Some(value) => Err(format!(
                "{} must be 'dual' or 'text', got '{value}'",
                Self::ENVIRONMENT_VARIABLE
            )),
        }
    }
}

impl ToolCallResult {
    #[must_use]
    pub fn success(value: Value) -> Self {
        Self::structured(value, false)
    }

    #[must_use]
    pub fn structured(value: Value, is_error: bool) -> Self {
        Self::structured_with_mode(value, is_error, ToolResponseMode::Dual)
    }

    #[must_use]
    pub(crate) fn success_with_mode(value: Value, mode: ToolResponseMode) -> Self {
        Self::structured_with_mode(value, false, mode)
    }

    #[must_use]
    pub(crate) fn structured_with_mode(
        value: Value,
        is_error: bool,
        mode: ToolResponseMode,
    ) -> Self {
        let text = value.to_string();
        Self {
            content: vec![TextContent {
                content_type: "text",
                text,
            }],
            structured_content: (mode == ToolResponseMode::Dual).then_some(value),
            is_error,
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

pub(super) fn object_schema(properties: &Value, required: &[&str]) -> Value {
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
    let mut tools = index::tools(&project_only);
    tools.extend(search::search_and_query_tools(&project));
    tools.extend(search::trace_and_source_tools(&project, &trace_schema));
    tools.extend(edit::tools(&project));
    tools.extend(compatibility::tools());
    tools
}

#[cfg(test)]
mod tests {
    use super::{ToolCallResult, ToolDefinition, ToolRegistry, ToolResponseMode};
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

    #[test]
    fn response_mode_parser_defaults_to_dual_and_rejects_unknown_values() {
        assert_eq!(
            ToolResponseMode::parse(None).expect("default response mode"),
            ToolResponseMode::Dual
        );
        assert_eq!(
            ToolResponseMode::parse(Some("dual")).expect("dual response mode"),
            ToolResponseMode::Dual
        );
        assert_eq!(
            ToolResponseMode::parse(Some("text")).expect("text response mode"),
            ToolResponseMode::Text
        );
        assert_eq!(
            ToolResponseMode::parse(Some("structured")).unwrap_err(),
            "GOLDENEYE_MCP_RESPONSE_MODE must be 'dual' or 'text', got 'structured'"
        );
    }

    #[test]
    fn text_response_mode_omits_only_the_duplicate_structured_content() {
        let payload = json!({"rows": [["alpha", 1]], "total": 1});
        let dual = serde_json::to_value(ToolCallResult::success(payload.clone()))
            .expect("serialize dual response");
        let text = serde_json::to_value(ToolCallResult::success_with_mode(
            payload.clone(),
            ToolResponseMode::Text,
        ))
        .expect("serialize text response");

        assert_eq!(dual["content"], text["content"]);
        assert_eq!(dual["isError"], text["isError"]);
        assert_eq!(dual["structuredContent"], payload);
        assert!(text.get("structuredContent").is_none());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                text["content"][0]["text"].as_str().expect("text content")
            )
            .expect("JSON text content"),
            dual["structuredContent"]
        );
    }

    #[test]
    fn error_responses_remain_text_only() {
        let value = serde_json::to_value(ToolCallResult::error("broken"))
            .expect("serialize error response");

        assert_eq!(value["content"][0]["text"], "broken");
        assert_eq!(value["isError"], true);
        assert!(value.get("structuredContent").is_none());
    }

    #[test]
    fn text_response_mode_preserves_structured_error_envelopes() {
        let payload = json!({"status": "not_found", "project": "missing"});
        let value = serde_json::to_value(ToolCallResult::structured_with_mode(
            payload.clone(),
            true,
            ToolResponseMode::Text,
        ))
        .expect("serialize structured error response");

        assert_eq!(value["isError"], true);
        assert!(value.get("structuredContent").is_none());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                value["content"][0]["text"].as_str().expect("text content")
            )
            .expect("JSON text content"),
            payload
        );
    }
}
