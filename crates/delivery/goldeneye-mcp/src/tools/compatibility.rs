use serde_json::json;

use super::ToolDefinition;

pub(super) fn tools() -> Vec<ToolDefinition> {
    vec![
        detect_changes_tool(),
        manage_adr_tool(),
        ingest_traces_tool(),
    ]
}

fn detect_changes_tool() -> ToolDefinition {
    ToolDefinition::new(
        "detect_changes",
        "Detect changes",
        "Detect code changes and their impact",
        json!({
            "type": "object",
            "properties": {
                "project": {"type": "string"},
                "scope": {"type": "string"},
                "depth": {"type": "integer", "default": 2},
                "base_branch": {"type": "string", "default": "main"},
                "since": {
                    "type": "string",
                    "description": "Git ref or date to compare from (e.g. HEAD~5, v0.5.0, 2026-01-01)"
                }
            },
            "required": ["project"]
        }),
    )
}

fn manage_adr_tool() -> ToolDefinition {
    ToolDefinition::new(
        "manage_adr",
        "Manage ADR",
        "Create or update Architecture Decision Records",
        json!({
            "type": "object",
            "properties": {
                "project": {"type": "string"},
                "mode": {"type": "string", "enum": ["get", "update", "sections"]},
                "content": {"type": "string"},
                "sections": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["project"]
        }),
    )
}

fn ingest_traces_tool() -> ToolDefinition {
    ToolDefinition::new(
        "ingest_traces",
        "Ingest traces",
        "Ingest runtime traces to enhance the knowledge graph",
        json!({
            "type": "object",
            "properties": {
                "traces": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "caller": {"type": "string"},
                            "callee": {"type": "string"},
                            "count": {"type": "integer"}
                        },
                        "additionalProperties": false
                    }
                },
                "project": {"type": "string"}
            },
            "required": ["traces", "project"]
        }),
    )
}
