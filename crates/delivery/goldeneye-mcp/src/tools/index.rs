use serde_json::{Value, json};

use super::{ToolDefinition, object_schema};

pub(super) fn tools(project_only: &Value) -> Vec<ToolDefinition> {
    vec![
        index_repository_tool(),
        list_projects_tool(),
        delete_project_tool(project_only),
        index_status_tool(project_only),
        graph_schema_tool(project_only),
    ]
}

fn index_repository_tool() -> ToolDefinition {
    ToolDefinition::new(
        "index_repository",
        "Index repository",
        "Index one allowed repository, persist its graph, or rebuild cross-project intelligence.",
        object_schema(
            &json!({
                "repo_path": {"type": "string", "description": "Path to the repository"},
                "mode": {"type": "string", "enum": ["full", "moderate", "fast", "cross-repo-intelligence"], "default": "full"},
                "target_projects": {"type": "array", "items": {"type": "string"}, "description": "Projects used by cross-repo intelligence; all projects are rebuilt when omitted"},
                "name": {"type": "string", "description": "Optional sanitized project-name override"},
                "persistence": {"type": "boolean", "default": false, "description": "Write a shared .codebase-memory artifact after indexing"}
            }),
            &["repo_path"],
        ),
    )
}

fn list_projects_tool() -> ToolDefinition {
    ToolDefinition::new(
        "list_projects",
        "List projects",
        "List persisted indexed projects.",
        object_schema(&json!({}), &[]),
    )
}

fn delete_project_tool(project_only: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "delete_project",
        "Delete project",
        "Delete one persisted project and all project-scoped graph data.",
        object_schema(project_only, &["project"]),
    )
}

fn index_status_tool(project_only: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "index_status",
        "Index status",
        "Return persisted graph counts and generation for one project.",
        project_only.clone(),
    )
}

fn graph_schema_tool(project_only: &Value) -> ToolDefinition {
    ToolDefinition::new(
        "get_graph_schema",
        "Get graph schema",
        "Return node labels, edge types, counts, and properties.",
        project_only.clone(),
    )
}
