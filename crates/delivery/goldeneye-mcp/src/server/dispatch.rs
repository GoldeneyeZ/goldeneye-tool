use super::errors::{
    parse_arguments, project_id, project_list_error, service_error_message, to_value,
};
use super::{
    ArchitectureArguments, CreateFileRequest, DeleteNodeRequest, EmptyArguments,
    GraphSchemaRequest, IndexArguments, IndexRepositoryMode, IndexRepositoryRequest,
    IndexStatusRequest, InspectSyntaxRequest, NodeContentRequest, ProjectArguments, QueryArguments,
    RequestId, SearchArguments, SearchCodeArguments, Server, SnippetArguments, TraceArguments,
    Value, json,
};

impl Server {
    // Keeping the dispatch table contiguous makes the MCP name-to-handler contract auditable.
    #[allow(clippy::too_many_lines)]
    pub(super) fn dispatch(
        &self,
        name: &str,
        arguments: Value,
        id: &RequestId,
    ) -> Result<Value, String> {
        match name {
            "index_repository" => self.dispatch_index_repository(name, arguments, id),
            "list_projects" => self.dispatch_list_projects(name, arguments),
            "delete_project" => self.dispatch_delete_project(name, arguments),
            "index_status" => self.dispatch_index_status(name, arguments),
            "get_graph_schema" => self.dispatch_get_graph_schema(name, arguments),
            "search_graph" => self.dispatch_search_graph(name, arguments),
            "search_code" => self.dispatch_search_code(name, arguments),
            "query_graph" => self.dispatch_query_graph(name, arguments),
            "trace_path" | "trace_call_path" => self.dispatch_trace_path(name, arguments),
            "get_code_snippet" => self.dispatch_get_code_snippet(name, arguments),
            "get_architecture" => self.dispatch_get_architecture(name, arguments),
            "inspect_syntax" => self.dispatch_inspect_syntax(name, arguments),
            "create_file" => self.dispatch_create_file(name, arguments),
            "replace_node" => self.dispatch_replace_node(name, arguments),
            "delete_node" => self.dispatch_delete_node(name, arguments),
            "insert_before_node" => self.dispatch_insert_before_node(name, arguments),
            "insert_after_node" => self.dispatch_insert_after_node(name, arguments),
            "manage_adr" => self.manage_adr(arguments),
            "ingest_traces" => self.ingest_traces(arguments),
            _ => Err(format!("Unknown tool: {name}")),
        }
    }

    fn dispatch_index_repository(
        &self,
        name: &str,
        arguments: Value,
        id: &RequestId,
    ) -> Result<Value, String> {
        let args: IndexArguments = parse_arguments(name, arguments)?;
        if args.mode.as_deref() == Some("cross-repo-intelligence") {
            let outcome = self
                .services()
                .rebuild_cross_repo_intelligence()
                .map_err(service_error_message)?;
            return Ok(json!({
                "mode": "cross-repo-intelligence",
                "projects": outcome.projects,
                "edges": outcome.edges,
                "target_projects": args.target_projects.unwrap_or_default(),
            }));
        }
        let mode = index_repository_mode(args.mode.as_deref())?;
        let mut request = IndexRepositoryRequest::new(args.repo_path)
            .with_mode(mode)
            .with_persistence(args.persistence);
        if let Some(name) = args.name {
            request = request.with_name(name);
        }
        self.index_repository(id, &request)
    }

    fn dispatch_list_projects(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let _: EmptyArguments = parse_arguments(name, arguments)?;
        self.list_projects()
    }

    fn dispatch_delete_project(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: ProjectArguments = parse_arguments(name, arguments)?;
        let project = project_id(name, args.project)?;
        let deleted = self
            .services()
            .delete_project(&project)
            .map_err(service_error_message)?;
        if deleted {
            let _ = self.watcher().unwatch(project.as_str());
            Ok(json!({ "project": project.as_str(), "status": "deleted" }))
        } else {
            Err(json!({ "project": project.as_str(), "status": "not_found" }).to_string())
        }
    }

    fn dispatch_index_status(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: ProjectArguments = parse_arguments(name, arguments)?;
        let project = project_id(name, args.project)?;
        let status = self
            .services()
            .index_status(&IndexStatusRequest::new(project))
            .map_err(service_error_message)?;
        to_value(json!({
            "project": status.project,
            "root_path": status.root_path,
            "generation": status.generation,
            "files": status.files,
            "nodes": status.nodes,
            "edges": status.edges,
            "query_only": status.query_only,
            "status": "ready"
        }))
    }

    fn dispatch_get_graph_schema(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: ProjectArguments = parse_arguments(name, arguments)?;
        let project = project_id(name, args.project)?;
        let schema = self
            .services()
            .get_graph_schema(&GraphSchemaRequest::new(project))
            .map_err(service_error_message)?;
        let labels = schema
            .node_labels
            .into_iter()
            .map(|entry| {
                json!({"label": entry.name, "count": entry.count, "properties": entry.properties})
            })
            .collect::<Vec<_>>();
        let edges = schema
            .edge_types
            .into_iter()
            .map(|entry| {
                json!({"type": entry.name, "count": entry.count, "properties": entry.properties})
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "project": schema.project,
            "schema_version": schema.schema_version,
            "node_labels": labels,
            "edge_types": edges
        }))
    }

    fn dispatch_search_graph(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: SearchArguments = parse_arguments(name, arguments)?;
        self.search_graph(args)
    }

    fn dispatch_search_code(&self, name: &str, arguments: Value) -> Result<Value, String> {
        if arguments.get("pattern").is_none() {
            return Err("pattern is required".to_owned());
        }
        if arguments.get("project").is_none() {
            return Err(project_list_error(self.services(), "project is required"));
        }
        let args: SearchCodeArguments = parse_arguments(name, arguments)?;
        self.search_code(args)
    }

    fn dispatch_query_graph(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: QueryArguments = parse_arguments(name, arguments)?;
        self.query_graph(args)
    }

    fn dispatch_trace_path(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: TraceArguments = parse_arguments(name, arguments)?;
        self.trace_path(args)
    }

    fn dispatch_get_code_snippet(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: SnippetArguments = parse_arguments(name, arguments)?;
        self.get_code_snippet(args)
    }

    fn dispatch_get_architecture(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args: ArchitectureArguments = parse_arguments(name, arguments)?;
        self.get_architecture(args)
    }

    fn dispatch_inspect_syntax(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: InspectSyntaxRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .inspect_syntax(&request)
                .map_err(service_error_message)?,
        )
    }

    fn dispatch_create_file(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: CreateFileRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .create_file(&request)
                .map_err(service_error_message)?,
        )
    }

    fn dispatch_replace_node(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: NodeContentRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .replace_node(&request)
                .map_err(service_error_message)?,
        )
    }

    fn dispatch_delete_node(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: DeleteNodeRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .delete_node(&request)
                .map_err(service_error_message)?,
        )
    }

    fn dispatch_insert_before_node(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: NodeContentRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .insert_before_node(&request)
                .map_err(service_error_message)?,
        )
    }

    fn dispatch_insert_after_node(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let request: NodeContentRequest = parse_arguments(name, arguments)?;
        to_value(
            self.services()
                .insert_after_node(&request)
                .map_err(service_error_message)?,
        )
    }
}

fn index_repository_mode(mode: Option<&str>) -> Result<IndexRepositoryMode, String> {
    match mode.unwrap_or("full") {
        "full" => Ok(IndexRepositoryMode::Full),
        "moderate" => Ok(IndexRepositoryMode::Moderate),
        "fast" => Ok(IndexRepositoryMode::Fast),
        mode => Err(format!(
            "Invalid parameters for index_repository: unsupported mode {mode}"
        )),
    }
}
