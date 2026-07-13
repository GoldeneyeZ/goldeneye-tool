use serde::Serialize;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

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
    #[cfg(test)]
    #[must_use]
    pub fn test(name: String) -> Self {
        Self {
            title: name.clone(),
            description: name.clone(),
            name,
            input_schema: json!({"type": "object"}),
            output_schema: json!({"type": "object", "additionalProperties": true}),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPage {
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
}

impl ToolRegistry {
    #[must_use]
    pub const fn new(tools: Vec<ToolDefinition>) -> Self {
        Self { tools }
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    content: Vec<TextContent>,
    is_error: bool,
}

impl ToolCallResult {
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent {
                content_type: "text",
                text: text.into(),
            }],
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

#[cfg(test)]
mod tests {
    use super::{ToolDefinition, ToolRegistry};
    use serde_json::json;

    #[test]
    fn registry_returns_all_without_cursor_and_pages_when_cursor_present() {
        let tools = (0..10)
            .map(|index| ToolDefinition::test(format!("tool-{index}")))
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
    fn empty_registry_advertises_no_tools() {
        let page = ToolRegistry::default().page(None).expect("empty page");

        assert!(page.tools.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn invalid_or_out_of_range_cursor_is_rejected() {
        let registry = ToolRegistry::new(vec![ToolDefinition::test("only".into())]);

        assert_eq!(
            registry.page(Some("not-a-number")).unwrap_err(),
            "invalid cursor"
        );
        assert_eq!(registry.page(Some("2")).unwrap_err(), "invalid cursor");
    }

    #[test]
    fn tool_definition_serializes_upstream_schema_fields() {
        let registry = ToolRegistry::new(vec![ToolDefinition::test("implemented".into())]);
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
