use std::path::PathBuf;

use goldeneye_query::QueryError;
use goldeneye_store::{RuntimeTrace, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ProjectId, ServiceError, Services};

pub const MAX_PERSISTED_TRACE_BATCH: usize = 1_024;
pub const MAX_TRACE_ENDPOINT_BYTES: usize = 1_024;

pub const ADR_EMPTY_HINT: &str = concat!(
    "No ADR yet. Create one with manage_adr(mode='update', ",
    "content='## PURPOSE\\n...\\n\\n## STACK\\n...\\n\\n## ARCHITECTURE\\n...",
    "\\n\\n## PATTERNS\\n...\\n\\n## TRADEOFFS\\n...\\n\\n## PHILOSOPHY\\n...'). ",
    "For guided creation: explore the codebase with get_architecture, ",
    "then draft and store. Sections: PURPOSE, STACK, ARCHITECTURE, ",
    "PATTERNS, TRADEOFFS, PHILOSOPHY."
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManageAdrRequest {
    pub project: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub sections: Vec<String>,
}

impl ManageAdrRequest {
    #[must_use]
    pub fn new(project: &ProjectId) -> Self {
        Self {
            project: project.as_str().to_owned(),
            mode: None,
            content: None,
            sections: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ManageAdrResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adr_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestTracesRequest {
    pub project: ProjectId,
    #[serde(default)]
    pub traces: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestTracesResult {
    pub status: &'static str,
    pub traces_received: usize,
    pub note: &'static str,
}

impl Services {
    /// Reads, updates, or lists headings from a project's durable ADR.
    ///
    /// # Errors
    ///
    /// Returns a typed project, path-policy, or storage error.
    pub fn manage_adr(&self, request: &ManageAdrRequest) -> Result<ManageAdrResult, ServiceError> {
        let (project, root) = self.project_and_root_for_reference(&request.project)?;
        let mut store = Store::open(self.config().database_path())?;
        let mut adr = store.get_adr(&project)?;
        if adr.is_none() {
            let legacy_path = root.join(".codebase-memory").join("adr.md");
            if let Ok(bytes) = std::fs::read(legacy_path)
                && !bytes.is_empty()
            {
                let legacy = String::from_utf8_lossy(&bytes).into_owned();
                store.store_adr(&project, &legacy)?;
                adr = store.get_adr(&project)?;
            }
        }

        let mode = request.mode.as_deref().unwrap_or("get");
        if matches!(mode, "update" | "store")
            && let Some(content) = request.content.as_deref()
        {
            store.store_adr(&project, content)?;
            return Ok(ManageAdrResult {
                status: Some("updated".to_owned()),
                ..ManageAdrResult::default()
            });
        }
        if mode == "sections" {
            return Ok(ManageAdrResult {
                sections: Some(adr_headers(
                    adr.as_ref().map(|record| record.content.as_str()),
                )),
                ..ManageAdrResult::default()
            });
        }
        if let Some(adr) = adr {
            return Ok(ManageAdrResult {
                content: Some(adr.content),
                ..ManageAdrResult::default()
            });
        }
        Ok(ManageAdrResult {
            content: Some(String::new()),
            status: Some("no_adr".to_owned()),
            adr_hint: Some(ADR_EMPTY_HINT.to_owned()),
            sections: None,
        })
    }

    /// Accepts runtime observations and durably aggregates the bounded valid prefix.
    ///
    /// The response deliberately preserves the upstream compatibility envelope. Runtime graph
    /// edge creation remains separate from storing the observations.
    ///
    /// # Errors
    ///
    /// Returns a typed project, path-policy, validation, or storage error.
    pub fn ingest_traces(
        &self,
        request: &IngestTracesRequest,
    ) -> Result<IngestTracesResult, ServiceError> {
        self.adr_project_root(&request.project)?;
        let mut store = Store::open(self.config().database_path())?;
        let traces = parse_runtime_traces(&request.traces);
        store.ingest_runtime_traces(&request.project, &traces)?;
        Ok(IngestTracesResult {
            status: "accepted",
            traces_received: request.traces.len(),
            note: "Runtime edge creation from traces not yet implemented",
        })
    }

    fn adr_project_root(&self, project: &ProjectId) -> Result<PathBuf, ServiceError> {
        self.prepare_database()?;
        let repository = self
            .dependencies
            .repositories()
            .open_query(self.config().database_path())
            .map_err(ServiceError::Repository)?;
        let record = repository
            .get_project(project)
            .map_err(ServiceError::Repository)?
            .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))?;
        self.resolve_repository(record.root_path.as_ref())
    }

    fn project_and_root_for_reference(
        &self,
        reference: &str,
    ) -> Result<(ProjectId, PathBuf), ServiceError> {
        let reference_path = std::path::Path::new(reference);
        let looks_like_path =
            reference_path.is_absolute() || reference.contains('/') || reference.contains('\\');
        if !looks_like_path {
            let project = ProjectId::new(reference).map_err(|error| ServiceError::Edit {
                code: crate::ServiceErrorCode::InvalidInput,
                message: format!("invalid project: {error}"),
            })?;
            let root = self.adr_project_root(&project)?;
            return Ok((project, root));
        }

        let canonical = self.resolve_repository(reference_path)?;
        self.prepare_database()?;
        let repository = self
            .dependencies
            .repositories()
            .open_query(self.config().database_path())
            .map_err(ServiceError::Repository)?;
        let project = repository
            .list_projects()
            .map_err(ServiceError::Repository)?
            .into_iter()
            .find_map(|record| {
                std::path::Path::new(&record.root_path)
                    .canonicalize()
                    .ok()
                    .filter(|root| *root == canonical)
                    .map(|_| record.id)
            })
            .ok_or_else(|| ServiceError::Edit {
                code: crate::ServiceErrorCode::NotFound,
                message: format!("project not found or not indexed: {reference}"),
            })?;
        Ok((project, canonical))
    }
}

#[must_use]
pub fn parse_runtime_traces(values: &[Value]) -> Vec<RuntimeTrace> {
    values
        .iter()
        .take(MAX_PERSISTED_TRACE_BATCH)
        .filter_map(parse_runtime_trace)
        .collect()
}

fn parse_runtime_trace(value: &Value) -> Option<RuntimeTrace> {
    let object = value.as_object()?;
    let source = object.get("caller")?.as_str()?;
    let target = object.get("callee")?.as_str()?;
    if source.is_empty()
        || target.is_empty()
        || source.len() > MAX_TRACE_ENDPOINT_BYTES
        || target.len() > MAX_TRACE_ENDPOINT_BYTES
    {
        return None;
    }
    let count = object.get("count").map_or(Some(1), Value::as_u64)?;
    if count == 0 || count > i64::MAX.cast_unsigned() {
        return None;
    }
    RuntimeTrace::new(source, target, count).ok()
}

fn adr_headers(content: Option<&str>) -> Vec<String> {
    content
        .into_iter()
        .flat_map(|content| content.split('\n'))
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            line.starts_with('#').then(|| truncate_header(line))
        })
        .collect()
}

fn truncate_header(header: &str) -> String {
    if header.len() <= 1_023 {
        return header.to_owned();
    }
    let mut end = 1_023;
    while !header.is_char_boundary(end) {
        end -= 1;
    }
    header[..end].to_owned()
}
