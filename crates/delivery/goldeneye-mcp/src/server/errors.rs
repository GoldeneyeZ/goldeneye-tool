use super::{
    BootstrapRuntime, DeserializeOwned, ProjectId, QueryError, QueryValue, Serialize, Server,
    ServiceConfig, ServiceError, ServiceErrorCode, Services, Value, json,
};

pub(super) fn response_mode_configuration_error(message: String) -> ServiceError {
    ServiceError::Edit {
        code: ServiceErrorCode::Configuration,
        message,
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::with_runtime(BootstrapRuntime::from_config(ServiceConfig::default()))
    }
}

pub(super) fn parse_arguments<T: DeserializeOwned>(name: &str, value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid parameters for {name}: {error}"))
}

pub(super) fn project_id(tool: &str, project: String) -> Result<ProjectId, String> {
    ProjectId::new(project)
        .map_err(|error| format!("Invalid parameters for {tool}: invalid project: {error}"))
}

pub(super) fn missing_project_error() -> String {
    json!({
        "error": "missing required argument: project",
        "hint": concat!(
            "Pass the project as the \"project\" argument, e.g. ",
            "{\"project\":\"<name from list_projects>\"}. ",
            "Run list_projects to see indexed projects."
        )
    })
    .to_string()
}

pub(super) fn compatibility_error(services: &Services, error: ServiceError) -> String {
    if matches!(
        error,
        ServiceError::Query(QueryError::ProjectNotFound(_))
            | ServiceError::Edit {
                code: ServiceErrorCode::NotFound,
                ..
            }
    ) {
        return project_list_error(services, "project not found or not indexed");
    }
    service_error_message(error)
}

pub(super) fn project_list_error(services: &Services, reason: &str) -> String {
    let projects = services
        .list_projects()
        .unwrap_or_default()
        .into_iter()
        .map(|project| project.project)
        .collect::<Vec<_>>();
    if projects.is_empty() {
        json!({
            "error": reason,
            "hint": "No projects indexed yet. Call index_repository first."
        })
        .to_string()
    } else {
        json!({
            "error": reason,
            "hint": concat!(
                "Use list_projects to see all indexed projects, then pass it as the ",
                "\"project\" argument."
            ),
            "available_projects": projects,
            "count": projects.len()
        })
        .to_string()
    }
}

pub(super) fn service_error_message(error: ServiceError) -> String {
    match error {
        ServiceError::Query(QueryError::ProjectNotFound(project)) => {
            format!("project not found or not indexed: {}", project.as_str())
        }
        ServiceError::Query(QueryError::AmbiguousSymbol {
            query,
            mut candidates,
        }) => {
            candidates.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            let names = candidates
                .into_iter()
                .map(|candidate| candidate.qualified_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("symbol is ambiguous: {query}; candidates: {names}")
        }
        ServiceError::Query(QueryError::SymbolNotFound {
            query,
            mut suggestions,
        }) => {
            suggestions.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            let names = suggestions
                .into_iter()
                .map(|suggestion| suggestion.qualified_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("symbol was not found: {query}; suggestions: {names}")
        }
        ServiceError::OutsideAllowedRoot => "repo_path is outside the allowed root".to_owned(),
        ServiceError::Cancelled => "Request cancelled".to_owned(),
        ServiceError::Edit { code, message } => {
            format!("{}: {message}", service_error_code(code))
        }
        other => other.to_string(),
    }
}

const fn service_error_code(code: ServiceErrorCode) -> &'static str {
    match code {
        ServiceErrorCode::Configuration => "configuration",
        ServiceErrorCode::InvalidInput => "invalid_input",
        ServiceErrorCode::Forbidden => "forbidden",
        ServiceErrorCode::NotFound => "not_found",
        ServiceErrorCode::Cancelled => "cancelled",
        ServiceErrorCode::Storage => "storage",
        ServiceErrorCode::Index => "index",
        ServiceErrorCode::Query => "query",
        ServiceErrorCode::Conflict => "conflict",
    }
}

pub(super) fn to_value(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("result serialization failed: {error}"))
}

pub(super) fn query_value(value: QueryValue) -> Result<Value, String> {
    match value {
        QueryValue::Null => Ok(Value::Null),
        QueryValue::Bool(value) => Ok(Value::Bool(value)),
        QueryValue::Integer(value) => Ok(json!(value)),
        QueryValue::Unsigned(value) => Ok(json!(value)),
        QueryValue::Float(value) => Ok(json!(value)),
        QueryValue::String(value) => Ok(Value::String(value)),
        QueryValue::Node(value) => to_value(value),
        QueryValue::Edge(value) => to_value(value),
        QueryValue::Json(value) => Ok(value),
    }
}
