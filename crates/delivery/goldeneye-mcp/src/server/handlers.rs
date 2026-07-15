use super::errors::{
    compatibility_error, missing_project_error, parse_arguments, project_id, query_value,
    service_error_message, to_value,
};
use super::{
    ArchitectureArguments, ArchitectureRequest, CancellationToken, CodeSnippetRequest,
    DetectChangesArguments, DetectChangesRequest, IndexRepositoryRequest, IndexStatusRequest,
    IngestTracesArguments, IngestTracesRequest, ManageAdrArguments, ManageAdrRequest,
    OperationHooks, PageRequest, ProjectId, QueryArguments, QueryError, QueryGraphRequest,
    RequestId, SearchArguments, SearchCodeArguments, SearchCodeRequest, SearchGraphRequest,
    SemanticSearchRequest, Server, ServiceError, SnippetArguments, TraceArguments, TraceDirection,
    TracePathRequest, Value, fs, json,
};

impl Server {
    pub(super) fn manage_adr(&self, arguments: Value) -> Result<Value, String> {
        let args: ManageAdrArguments = parse_arguments("manage_adr", arguments)?;
        let Some(project) = args.project else {
            return Err(missing_project_error());
        };
        let request = ManageAdrRequest {
            project,
            mode: args.mode,
            content: args.content,
            sections: args.sections,
        };
        let result = self
            .services()
            .manage_adr(&request)
            .map_err(|error| compatibility_error(self.services(), error))?;
        to_value(result)
    }

    pub(super) fn ingest_traces(&self, arguments: Value) -> Result<Value, String> {
        let args: IngestTracesArguments = parse_arguments("ingest_traces", arguments)?;
        let traces_received = args.traces.len();
        let Some(project) = args.project else {
            return Ok(json!({
                "status": "accepted",
                "traces_received": traces_received,
                "note": "Runtime edge creation from traces not yet implemented"
            }));
        };
        let request = IngestTracesRequest {
            project: project_id("ingest_traces", project)?,
            traces: args.traces,
        };
        let result = self
            .services()
            .ingest_traces(&request)
            .map_err(|error| compatibility_error(self.services(), error))?;
        to_value(result)
    }

    pub(super) fn detect_changes(
        &self,
        arguments: Value,
        id: &RequestId,
    ) -> Result<(Value, bool), String> {
        let request = detect_changes_request(arguments)?;
        let cancellation = CancellationToken::new();
        {
            let mut active = self
                .active_index
                .lock()
                .map_err(|_| "change cancellation state is unavailable".to_owned())?;
            *active = Some((id.clone(), cancellation.clone()));
        }
        let result = self.services().detect_changes(&request, &cancellation);
        if let Ok(mut active) = self.active_index.lock()
            && active
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == id)
        {
            *active = None;
        }
        let result = result.map_err(|error| compatibility_error(self.services(), error))?;
        let is_error = result.is_error;
        Ok((to_value(result)?, is_error))
    }

    pub(super) fn index_repository(
        &self,
        id: &RequestId,
        request: &IndexRepositoryRequest,
    ) -> Result<Value, String> {
        let cancellation = CancellationToken::new();
        {
            let mut active = self
                .active_index
                .lock()
                .map_err(|_| "index cancellation state is unavailable".to_owned())?;
            *active = Some((id.clone(), cancellation.clone()));
        }
        let result = self
            .services()
            .index_repository_with_hooks(request, &OperationHooks::new(cancellation));
        if let Ok(mut active) = self.active_index.lock()
            && active
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == id)
        {
            *active = None;
        }
        let result = result.map_err(service_error_message)?;
        let _ = self.watcher().watch(&result.project, &result.root_path);
        to_value(result)
    }

    pub(super) fn list_projects(&self) -> Result<Value, String> {
        let projects = self
            .services()
            .list_projects()
            .map_err(service_error_message)?;
        let database_bytes = fs::metadata(self.services().config().database_path())
            .map_or(0, |metadata| metadata.len());
        let mut rows = Vec::with_capacity(projects.len());
        for project in projects {
            let id = ProjectId::new(project.project.clone())
                .map_err(|error| format!("stored project name is invalid: {error}"))?;
            let status = self
                .services()
                .index_status(&IndexStatusRequest::new(id))
                .map_err(service_error_message)?;
            rows.push(json!({
                "name": project.project,
                "root_path": project.root_path,
                "generation": project.generation,
                "nodes": status.nodes,
                "edges": status.edges,
                "size_bytes": database_bytes
            }));
        }
        Ok(json!({"projects": rows}))
    }

    pub(super) fn search_graph(&self, args: SearchArguments) -> Result<Value, String> {
        let plan = search_graph_request(args)?;
        let page = self
            .services()
            .search_graph(&plan.request)
            .map_err(service_error_message)?;
        let mut value = to_value(page)?;
        value
            .as_object_mut()
            .ok_or_else(|| "search serialization did not produce an object".to_owned())?
            .insert(
                "search_mode".to_owned(),
                Value::String(plan.search_mode.to_owned()),
            );
        self.append_semantic_results(
            &mut value,
            plan.project,
            plan.semantic_query,
            plan.semantic_limit,
        )?;
        Ok(value)
    }

    fn append_semantic_results(
        &self,
        value: &mut Value,
        project: ProjectId,
        semantic_query: Option<Vec<String>>,
        limit: usize,
    ) -> Result<(), String> {
        let Some(keywords) = semantic_query else {
            return Ok(());
        };
        let mut request = SemanticSearchRequest::new(project, keywords);
        request.limit = limit;
        let semantic = self
            .services()
            .semantic_search(&request)
            .map_err(service_error_message)?;
        let results = semantic
            .results
            .into_iter()
            .map(|hit| {
                json!({
                    "name": hit.node.name,
                    "qualified_name": hit.node.qualified_name,
                    "label": hit.node.label,
                    "file_path": hit.node.file_path,
                    "score": hit.score
                })
            })
            .collect::<Vec<_>>();
        value
            .as_object_mut()
            .expect("search serialization was checked as an object")
            .insert("semantic_results".to_owned(), Value::Array(results));
        Ok(())
    }

    pub(super) fn query_graph(&self, args: QueryArguments) -> Result<Value, String> {
        let project = project_id("query_graph", args.project)?;
        let mut request = QueryGraphRequest::new(project, args.query);
        request.max_rows = args.max_rows;
        let result = self
            .services()
            .query_graph(&request)
            .map_err(service_error_message)?;
        let rows = result
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(query_value)
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({
            "project": result.project,
            "columns": result.columns,
            "rows": rows,
            "total": result.total,
            "truncated": result.truncated
        }))
    }

    pub(super) fn search_code(&self, args: SearchCodeArguments) -> Result<Value, String> {
        let project = project_id("search_code", args.project)?;
        let regex = args.regex;
        let mut request = SearchCodeRequest::new(project, args.pattern);
        request.file_pattern = args.file_pattern;
        request.path_filter = args.path_filter;
        request.mode = args.mode;
        request.context = args.context;
        request.regex = regex;
        request.limit = args.limit;
        match self.services().search_code(&request) {
            Ok(result) => to_value(result),
            Err(ServiceError::Query(QueryError::InvalidPattern {
                field: "pattern", ..
            })) if regex => Err(
                "invalid regex pattern (regex=true): check for unbalanced (), [], or {}".to_owned(),
            ),
            Err(error) => Err(compatibility_error(self.services(), error)),
        }
    }

    pub(super) fn trace_path(&self, args: TraceArguments) -> Result<Value, String> {
        if args.mode.as_deref().is_some_and(|mode| mode != "calls") {
            return Err(
                "Invalid parameters for trace_path: only calls mode is supported".to_owned(),
            );
        }
        let project = project_id("trace_path", args.project)?;
        let mut request = TracePathRequest::new(project, args.function_name, args.direction);
        request.depth = args.depth;
        request.edge_types = args.edge_types;
        let result = self
            .services()
            .trace_path(&request)
            .map_err(service_error_message)?;
        let mut value = to_value(&result)?;
        add_trace_aliases(&mut value, result.direction)?;
        Ok(value)
    }

    pub(super) fn get_code_snippet(&self, args: SnippetArguments) -> Result<Value, String> {
        let project = project_id("get_code_snippet", args.project)?;
        let result = self
            .services()
            .get_code_snippet(&CodeSnippetRequest::new(project, args.qualified_name))
            .map_err(service_error_message)?;
        let Value::Object(mut object) = to_value(&result.symbol)? else {
            return Err("snippet symbol serialization did not produce an object".to_owned());
        };
        object.insert("project".to_owned(), Value::String(result.project));
        object.insert("source".to_owned(), Value::String(result.source.clone()));
        object.insert("code".to_owned(), Value::String(result.source));
        object.insert("file_path".to_owned(), Value::String(result.file_path));
        object.insert("start_byte".to_owned(), json!(result.start_byte));
        object.insert("end_byte".to_owned(), json!(result.end_byte));
        object.insert("start_line".to_owned(), json!(result.start_line));
        object.insert("end_line".to_owned(), json!(result.end_line));
        object.insert(
            "content_hash".to_owned(),
            Value::String(result.content_hash),
        );
        Ok(Value::Object(object))
    }

    pub(super) fn get_architecture(&self, args: ArchitectureArguments) -> Result<Value, String> {
        let project = project_id("get_architecture", args.project)?;
        let result = self
            .services()
            .get_architecture(&ArchitectureRequest::new(project))
            .map_err(service_error_message)?;
        Ok(json!({
            "project": result.project,
            "root_path": result.root_path,
            "generation": result.generation,
            "total_nodes": result.total_nodes,
            "total_edges": result.total_edges,
            "languages": result.languages,
            "packages": result.modules,
            "types": result.types,
            "entry_points": result.entry_points,
            "edge_types": result.edge_types,
            "hotspots": [],
            "boundaries": [],
            "layers": [],
            "clusters": []
        }))
    }
}

struct SearchGraphDispatch {
    project: ProjectId,
    request: SearchGraphRequest,
    semantic_query: Option<Vec<String>>,
    semantic_limit: usize,
    search_mode: &'static str,
}

fn search_graph_request(args: SearchArguments) -> Result<SearchGraphDispatch, String> {
    let project = project_id("search_graph", args.project)?;
    let semantic_query = args.semantic_query;
    let search_mode = if args.query.is_some() {
        "bm25"
    } else {
        "regex"
    };
    let mut request = SearchGraphRequest::new(project.clone());
    request.query = args.query;
    request.name_pattern = args.name_pattern;
    request.qualified_name_pattern = args.qn_pattern;
    request.label = args.label;
    request.file_pattern = args.file_pattern;
    request.relationship = args.relationship;
    request.min_degree = args.min_degree;
    request.max_degree = args.max_degree;
    request.exclude_entry_points = args.exclude_entry_points;
    request.include_connected = args.include_connected;
    request.page = PageRequest {
        limit: args.limit,
        offset: args.offset,
        cursor: args.cursor,
    };
    Ok(SearchGraphDispatch {
        project,
        request,
        semantic_query,
        semantic_limit: args.limit,
        search_mode,
    })
}

fn detect_changes_request(arguments: Value) -> Result<DetectChangesRequest, String> {
    let args: DetectChangesArguments = parse_arguments("detect_changes", arguments)?;
    let Some(project) = args.project else {
        return Err(missing_project_error());
    };
    let mut request = DetectChangesRequest::new(project_id("detect_changes", project)?);
    request.scope = args.scope;
    request.depth = args
        .depth
        .unwrap_or(goldeneye_services::DEFAULT_CHANGE_DEPTH);
    request.base_branch = args.base_branch.unwrap_or_else(|| "main".to_owned());
    request.since = args.since;
    Ok(request)
}

fn add_trace_aliases(value: &mut Value, direction: TraceDirection) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "trace serialization did not produce an object".to_owned())?;
    let paths = object
        .get("paths")
        .cloned()
        .ok_or_else(|| "trace serialization did not produce paths".to_owned())?;
    match direction {
        TraceDirection::Inbound => {
            object.insert("callers".to_owned(), paths);
        }
        TraceDirection::Outbound => {
            object.insert("callees".to_owned(), paths);
        }
        TraceDirection::Both => {
            object.insert("callers".to_owned(), paths.clone());
            object.insert("callees".to_owned(), paths);
        }
    }
    Ok(())
}
