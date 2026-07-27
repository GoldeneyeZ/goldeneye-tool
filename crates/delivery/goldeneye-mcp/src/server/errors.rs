use super::{
    BootstrapRuntime, DeserializeOwned, ProjectId, QueryError, QueryValue, Serialize, Server,
    ServiceConfig, ServiceError, ServiceErrorCode, Services, ToolCallResult, Value, json,
};

pub(super) struct SnippetToolError {
    message: String,
    structured_content: Option<Value>,
}

impl SnippetToolError {
    pub(super) fn untyped(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            structured_content: None,
        }
    }

    pub(super) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let reason = bounded_reason(&reason);
        let message = format!("{field} is invalid: {reason}");
        Self::typed(
            message,
            "InvalidSnippetArguments",
            &json!({"field": field, "reason": reason}),
        )
    }

    pub(super) fn from_service(error: ServiceError) -> Self {
        let structured = snippet_error_details(&error);
        let message = service_error_message(error);
        match structured {
            Some((code, details)) => Self::typed(message, code, &details),
            None => Self::untyped(message),
        }
    }

    pub(super) fn from_legacy_service(error: ServiceError) -> Self {
        let structured = legacy_snippet_error_details(&error);
        let message = service_error_message(error);
        match structured {
            Some((code, details)) => Self::typed(message, code, &details),
            None => Self::untyped(message),
        }
    }

    pub(super) fn into_tool_call_result(self) -> ToolCallResult {
        match self.structured_content {
            Some(structured_content) => {
                ToolCallResult::typed_error(self.message, structured_content)
            }
            None => ToolCallResult::error(self.message),
        }
    }

    fn typed(message: String, code: &'static str, details: &Value) -> Self {
        let structured_content = json!({
            "code": code,
            "message": message,
            "details": details,
        });
        Self {
            message,
            structured_content: Some(structured_content),
        }
    }
}

fn snippet_error_details(error: &ServiceError) -> Option<(&'static str, Value)> {
    let ServiceError::Query(error) = error else {
        return None;
    };
    match error {
        QueryError::SnippetTooLarge {
            actual_bytes,
            actual_lines,
            maximum_bytes,
            maximum_lines,
        } => Some((
            "SnippetTooLarge",
            json!({
                "actual_bytes": actual_bytes,
                "actual_lines": actual_lines,
                "maximum_bytes": maximum_bytes,
                "maximum_lines": maximum_lines,
            }),
        )),
        QueryError::InvalidSnippetArguments { field, reason } => Some((
            "InvalidSnippetArguments",
            json!({"field": field, "reason": reason}),
        )),
        QueryError::InvalidSnippetChunkBytes {
            actual,
            minimum,
            maximum,
        } => Some((
            "SnippetChunkBytesOutOfRange",
            json!({"actual": actual, "minimum": minimum, "maximum": maximum}),
        )),
        QueryError::InvalidSnippetChunk {
            actual,
            chunk_count,
        } => Some((
            "SnippetChunkOutOfRange",
            json!({"actual": actual, "chunk_count": chunk_count}),
        )),
        QueryError::StaleSnippetSource {
            expected_source_sha256,
            actual_source_sha256,
        } => Some((
            "StaleSnippetSource",
            json!({
                "expected_source_sha256": expected_source_sha256,
                "actual_source_sha256": actual_source_sha256,
            }),
        )),
        QueryError::StaleFile {
            path,
            expected_hash,
            actual_hash,
        } => Some((
            "StaleIndexedFile",
            json!({
                "file_path": path,
                "expected_indexed_file_hash": expected_hash,
                "actual_file_hash": actual_hash,
            }),
        )),
        QueryError::SourceNotUtf8 { qualified_name } => Some((
            "SnippetSourceNotUtf8",
            json!({"qualified_name": qualified_name}),
        )),
        QueryError::CorruptSourceSpan { qualified_name } => Some((
            "InconsistentSnippetIndex",
            json!({
                "qualified_name": qualified_name,
                "reason": "source span is outside file bounds",
            }),
        )),
        _ => None,
    }
}

fn legacy_snippet_error_details(error: &ServiceError) -> Option<(&'static str, Value)> {
    let ServiceError::Query(QueryError::SnippetTooLarge {
        actual_bytes,
        actual_lines,
        maximum_bytes,
        maximum_lines,
    }) = error
    else {
        return None;
    };
    Some((
        "SnippetTooLarge",
        json!({
            "actual_bytes": actual_bytes,
            "actual_lines": actual_lines,
            "maximum_bytes": maximum_bytes,
            "maximum_lines": maximum_lines,
        }),
    ))
}

fn bounded_reason(reason: &str) -> String {
    const MAX_REASON_CHARS: usize = 512;
    let mut chars = reason.chars();
    let bounded = chars.by_ref().take(MAX_REASON_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

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
