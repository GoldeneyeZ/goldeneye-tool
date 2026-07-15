use serde_json::{Value, json};

use super::{ToolDefinition, object_schema};

pub(super) fn search_and_query_tools(project: &Value) -> Vec<ToolDefinition> {
    vec![
        search_graph_tool(project),
        search_code_tool(project),
        query_graph_tool(project),
    ]
}

fn search_graph_tool(project: &Value) -> ToolDefinition {
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
                "semantic_query": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Keyword array scored independently using per-keyword minimum cosine"
                },
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
    )
}

fn search_code_tool(project: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "search_code",
        "Search code",
        "Graph-augmented code search. Finds text patterns, deduplicates matches into their \
         containing functions, and ranks structural definitions before tests. compact returns \
         signatures and metadata; full adds a match-anchored source window capped at 60 lines; \
         files returns only paths. Compare total_results with limit to detect truncation.",
        object_schema(
            &json!({
                "pattern": {"type": "string"},
                "project": project.clone(),
                "file_pattern": {
                    "type": "string",
                    "description": "Glob for included file names (for example *.go)"
                },
                "path_filter": {
                    "type": "string",
                    "description": "Regex filter on indexed file paths (for example ^src/)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["compact", "full", "files"],
                    "default": "compact"
                },
                "context": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Context lines around matches in compact mode"
                },
                "regex": {"type": "boolean", "default": false},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 10}
            }),
            &["pattern", "project"],
        ),
    )
}

fn query_graph_tool(project: &Value) -> ToolDefinition {
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
    )
}

pub(super) fn trace_and_source_tools(project: &Value, trace_schema: &Value) -> Vec<ToolDefinition> {
    vec![
        trace_path_tool(trace_schema),
        trace_call_path_tool(trace_schema),
        code_snippet_tool(project),
        architecture_tool(project),
    ]
}

fn trace_path_tool(trace_schema: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "trace_path",
        "Trace path",
        "Trace CALLS relationships inbound, outbound, or both.",
        trace_schema.clone(),
    )
}

fn trace_call_path_tool(trace_schema: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "trace_call_path",
        "Trace call path",
        "Compatibility alias for trace_path.",
        trace_schema.clone(),
    )
}

fn code_snippet_tool(project: &Value) -> ToolDefinition {
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
    )
}

fn architecture_tool(project: &Value) -> ToolDefinition {
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
    )
}
