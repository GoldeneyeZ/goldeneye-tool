use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_domain::{GraphEdge, GraphNode, ProjectId};
use goldeneye_git::GitCommandRepository;
use goldeneye_mcp::server::Server;
use goldeneye_services::{
    IndexRepositoryMode, IndexRepositoryRequest, ServiceConfig, ServiceDependencies, Services,
};
use goldeneye_store::{QueryStore, Store};
use goldeneye_watcher::{ServiceIndexer, WatchRuntime, Watcher, WatcherConfig};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

const MAX_INDEX_JOBS: usize = 4;
const MAX_LAYOUT_NODES: usize = 250_000;
const DEFAULT_LAYOUT_NODES: usize = 5_000;
const MAX_LOG_LINES: usize = 2_000;
const LOG_CAPACITY: usize = 4_096;

fn service_dependencies() -> ServiceDependencies {
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
    )
}

pub trait ApiBackend: Send + Sync + 'static {
    /// # Errors
    ///
    /// Returns a typed HTTP error when request validation or backend processing fails.
    fn handle(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl ApiRequest {
    /// Decodes the request body as JSON.
    ///
    /// # Errors
    ///
    /// Returns HTTP 400 when the body is not valid JSON for the requested type.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        serde_json::from_slice(&self.body).map_err(|_| ApiError::new(400, "invalid JSON body"))
    }

    /// Returns a non-empty query parameter.
    ///
    /// # Errors
    ///
    /// Returns HTTP 400 when the parameter is absent or empty.
    pub fn required_query(&self, key: &str) -> Result<&str, ApiError> {
        self.query
            .get(key)
            .filter(|value| !value.is_empty())
            .map(String::as_str)
            .ok_or_else(|| ApiError::new(400, format!("missing {key} parameter")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
}

impl ApiResponse {
    #[must_use]
    pub const fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    #[must_use]
    pub const fn ok(body: Value) -> Self {
        Self::new(200, body)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    #[must_use]
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

pub struct GoldeneyeBackend {
    config: ServiceConfig,
    rpc: Server,
    jobs: Arc<Mutex<Vec<IndexJob>>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    watcher: Arc<Watcher<ServiceIndexer>>,
    watcher_runtime: Mutex<Option<WatchRuntime>>,
    started: Instant,
}

impl GoldeneyeBackend {
    #[must_use]
    pub fn new(config: ServiceConfig) -> Self {
        let watcher = Arc::new(Watcher::new(
            WatcherConfig::default(),
            ServiceIndexer::new(config.clone()),
        ));
        if config.database_path().is_file()
            && let Ok(store) = Store::open_read_only(config.database_path())
            && let Ok(projects) = store.list_projects()
        {
            for project in projects {
                let _ = watcher.watch(project.id.as_str(), project.root_path);
            }
        }
        let watcher_runtime = watcher.spawn().ok();
        Self {
            rpc: Server::new(Services::new(config.clone(), service_dependencies())),
            config,
            jobs: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            watcher,
            watcher_runtime: Mutex::new(watcher_runtime),
            started: Instant::now(),
        }
    }

    /// Builds the HTTP backend using process environment configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed service configuration error.
    pub fn from_env() -> Result<Self, goldeneye_services::ServiceError> {
        Ok(Self::new(ServiceConfig::from_env()?))
    }

    fn handle_rpc(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let line = std::str::from_utf8(&request.body)
            .map_err(|_| ApiError::new(400, "RPC body is not UTF-8"))?;
        let response = self
            .rpc
            .handle_line(line)
            .ok_or_else(|| ApiError::new(400, "RPC notifications have no HTTP response"))?;
        let body = serde_json::to_value(response)
            .map_err(|error| ApiError::new(500, format!("RPC serialization failed: {error}")))?;
        Ok(ApiResponse::ok(body))
    }

    fn handle_layout(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let requested_project = request.required_query("project")?;
        let project_name = if request
            .query
            .get("graph")
            .is_some_and(|value| value == "missed")
        {
            format!("{requested_project}::missed")
        } else {
            requested_project.to_owned()
        };
        let max_nodes = request
            .query
            .get("max_nodes")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| ApiError::new(400, "invalid max_nodes parameter"))?
            .unwrap_or(DEFAULT_LAYOUT_NODES)
            .clamp(1, MAX_LAYOUT_NODES);
        let store = self.query_store()?;
        let project = project_id(&project_name)?;
        if store.get_project(&project).map_err(internal)?.is_none() {
            return Err(ApiError::new(404, "project not found"));
        }
        let mut body = layout_value(&store, &project, max_nodes)?;

        if project_name == requested_project {
            let linked_projects = linked_projects_value(&store, &project, max_nodes)?;
            if !linked_projects.is_empty()
                && let Some(object) = body.as_object_mut()
            {
                object.insert("linked_projects".to_owned(), Value::Array(linked_projects));
            }
            let missed = project_id(&format!("{requested_project}::missed"))?;
            if store.get_project(&missed).map_err(internal)?.is_some()
                && let Some(object) = body.as_object_mut()
            {
                object.insert(
                    "missed_graph".to_owned(),
                    layout_value(&store, &missed, max_nodes)?,
                );
            }
        }
        Ok(ApiResponse::ok(body))
    }

    fn handle_repo_info(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let project = project_id(request.required_query("project")?)?;
        let store = self.query_store()?;
        let record = store
            .get_project(&project)
            .map_err(internal)?
            .ok_or_else(|| ApiError::new(404, "project not found"))?;
        let root = PathBuf::from(&record.root_path);
        let branch = git_output(&root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .filter(|branch| branch != "HEAD")
            .unwrap_or_default();
        let remote = git_output(&root, &["remote", "get-url", "origin"]).unwrap_or_default();
        let remote_url = strip_remote_credentials(&remote);
        let web_base = remote_web_base(&remote_url).unwrap_or_default();
        let blob_base = if web_base.is_empty() || branch.is_empty() {
            String::new()
        } else {
            format!("{web_base}/blob/{}", encode_url_path(&branch))
        };
        Ok(ApiResponse::ok(json!({
            "root_path": record.root_path,
            "branch": branch,
            "remote_url": remote_url,
            "web_base": web_base,
            "blob_base": blob_base,
        })))
    }

    fn handle_index(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        #[derive(serde::Deserialize)]
        struct Body {
            root_path: PathBuf,
            #[serde(default)]
            project_name: String,
            #[serde(default)]
            persistence: bool,
            #[serde(default)]
            mode: IndexRepositoryMode,
        }

        let body: Body = request.json()?;
        let name_override = (!body.project_name.is_empty()).then(|| body.project_name.clone());
        let root = body
            .root_path
            .canonicalize()
            .map_err(|_| ApiError::new(400, "directory not found"))?;
        if !root.is_dir() {
            return Err(ApiError::new(400, "directory not found"));
        }
        self.authorize_path(&root)?;

        let mut jobs = lock(&self.jobs)?;
        if jobs
            .iter()
            .any(|job| job.status == JobStatus::Indexing && job.path == root)
        {
            return Err(ApiError::new(409, "repository is already indexing"));
        }
        let slot = jobs
            .iter()
            .position(|job| job.status != JobStatus::Indexing)
            .or_else(|| (jobs.len() < MAX_INDEX_JOBS).then_some(jobs.len()))
            .ok_or_else(|| ApiError::new(429, "all index slots busy"))?;
        let job = IndexJob {
            slot,
            status: JobStatus::Indexing,
            path: root.clone(),
            project_name: body.project_name,
            error: String::new(),
        };
        if slot == jobs.len() {
            jobs.push(job);
        } else {
            jobs[slot] = job;
        }
        drop(jobs);

        self.log(&format!("index[{slot}] started {}", root.display()));
        let jobs = Arc::clone(&self.jobs);
        let logs = Arc::clone(&self.logs);
        let watcher = Arc::clone(&self.watcher);
        let config = self.config.clone();
        let thread_root = root.clone();
        thread::Builder::new()
            .name(format!("goldeneye-index-{slot}"))
            .spawn(move || {
                let result = Services::new(config.clone(), service_dependencies())
                    .index_repository(&IndexRepositoryRequest {
                        repo_path: thread_root.clone(),
                        name: name_override,
                        mode: body.mode,
                        persistence: body.persistence || persistence_enabled(),
                    });
                let (status, error) = match &result {
                    Ok(indexed) => {
                        let _ = watcher.watch(&indexed.project, &thread_root);
                        (JobStatus::Done, String::new())
                    }
                    Err(error) => (JobStatus::Error, error.to_string()),
                };
                if let Ok(mut jobs) = jobs.lock()
                    && let Some(job) = jobs.get_mut(slot)
                {
                    job.status = status;
                    job.error.clone_from(&error);
                }
                let message = format!(
                    "index[{slot}] {} {}",
                    status.as_str(),
                    if error.is_empty() {
                        thread_root.display().to_string()
                    } else {
                        error
                    }
                );
                push_log(&logs, &message);
            })
            .map_err(|error| ApiError::new(500, format!("thread creation failed: {error}")))?;

        Ok(ApiResponse::new(
            202,
            json!({ "status": "indexing", "slot": slot, "path": root }),
        ))
    }

    fn handle_index_status(&self) -> Result<ApiResponse, ApiError> {
        let jobs = lock(&self.jobs)?;
        let entries = jobs
            .iter()
            .map(|job| {
                json!({
                    "slot": job.slot,
                    "status": job.status.as_str(),
                    "path": job.path,
                    "project_name": job.project_name,
                    "error": job.error,
                })
            })
            .collect::<Vec<_>>();
        Ok(ApiResponse::ok(json!({ "jobs": entries })))
    }

    fn handle_ui_config(request: &ApiRequest) -> ApiResponse {
        let language = std::env::var("CBM_UI_LANG")
            .ok()
            .filter(|value| matches!(value.as_str(), "en" | "fr"))
            .unwrap_or_else(|| {
                request
                    .headers
                    .get("accept-language")
                    .filter(|value| value.to_ascii_lowercase().starts_with("fr"))
                    .map_or_else(|| "en".to_owned(), |_| "fr".to_owned())
            });
        ApiResponse::ok(json!({
            "lang": language,
            "upstream_issues_url": "https://github.com/GoldeneyeZ/goldeneye-tool/issues/new",
        }))
    }

    fn handle_delete_project(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let project = project_id(request.required_query("name")?)?;
        if !self.config.database_path().is_file() {
            return Err(ApiError::new(404, "project not found"));
        }
        let mut store = Store::open(self.config.database_path()).map_err(internal)?;
        if !store.delete_project(&project).map_err(internal)? {
            return Err(ApiError::new(404, "project not found"));
        }
        let _ = self.watcher.unwatch(project.as_str());
        self.log(&format!("project deleted {}", project.as_str()));
        Ok(ApiResponse::ok(json!({ "deleted": true })))
    }

    fn handle_browse(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let raw = request.required_query("path")?;
        let path = PathBuf::from(raw)
            .canonicalize()
            .map_err(|_| ApiError::new(400, "not a directory"))?;
        if !path.is_dir() {
            return Err(ApiError::new(400, "not a directory"));
        }
        self.authorize_path(&path)?;
        let mut directories = fs::read_dir(&path)
            .map_err(|_| ApiError::new(403, "cannot open directory"))?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(std::fs::FileType::is_dir)
                    .map(|_| entry.file_name().to_string_lossy().into_owned())
            })
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        directories.sort_by_key(|name| name.to_ascii_lowercase());

        let allowed = self.allowed_root()?;
        let parent = path
            .parent()
            .filter(|parent| parent.starts_with(&allowed))
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(ApiResponse::ok(json!({
            "path": path,
            "dirs": directories,
            "parent": parent,
        })))
    }

    fn handle_adr_get(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let project = project_id(request.required_query("project")?)?;
        let store = self.query_store()?;
        let Some(adr) = store.get_adr(&project).map_err(internal)? else {
            return Ok(ApiResponse::ok(json!({ "has_adr": false })));
        };
        Ok(ApiResponse::ok(json!({
            "has_adr": true,
            "content": adr.content,
            "updated_at": adr.updated_at,
        })))
    }

    fn handle_adr_save(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        #[derive(serde::Deserialize)]
        struct Body {
            project: String,
            content: String,
        }
        let body: Body = request.json()?;
        if body.content.len() > 64 * 1_024 {
            return Err(ApiError::new(413, "ADR content too large"));
        }
        let project = project_id(&body.project)?;
        let mut store = Store::open(self.config.database_path()).map_err(internal)?;
        if store.get_project(&project).map_err(internal)?.is_none() {
            return Err(ApiError::new(404, "project not found"));
        }
        store.store_adr(&project, &body.content).map_err(internal)?;
        Ok(ApiResponse::ok(json!({ "saved": true })))
    }

    fn handle_project_health(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let project = project_id(request.required_query("name")?)?;
        if !self.config.database_path().is_file() {
            return Ok(ApiResponse::ok(json!({ "status": "missing" })));
        }
        let store = match Store::open_read_only(self.config.database_path()) {
            Ok(store) => store,
            Err(error) => {
                return Ok(ApiResponse::ok(json!({
                    "status": "corrupt",
                    "reason": error.to_string(),
                })));
            }
        };
        if store.get_project(&project).map_err(internal)?.is_none() {
            return Ok(ApiResponse::ok(json!({ "status": "missing" })));
        }
        let counts = store.counts(&project).map_err(internal)?;
        let size = fs::metadata(self.config.database_path()).map_or(0, |metadata| metadata.len());
        Ok(ApiResponse::ok(json!({
            "status": "healthy",
            "nodes": counts.nodes,
            "edges": counts.edges,
            "size_bytes": size,
        })))
    }

    fn handle_processes(&self) -> ApiResponse {
        let pid = std::process::id();
        let elapsed = format_elapsed(self.started.elapsed().as_secs());
        let command = std::env::args().collect::<Vec<_>>().join(" ");
        let rss = current_rss_megabytes();
        ApiResponse::ok(json!({
            "self_rss_mb": rss,
            "self_user_cpu_s": 0.0,
            "self_sys_cpu_s": 0.0,
            "processes": [{
                "pid": pid,
                "cpu": 0.0,
                "rss_mb": rss,
                "elapsed": elapsed,
                "command": command,
                "is_self": true,
            }],
        }))
    }

    fn handle_logs(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let requested = request
            .query
            .get("lines")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| ApiError::new(400, "invalid lines parameter"))?
            .unwrap_or(200)
            .min(MAX_LOG_LINES);
        let logs = lock(&self.logs)?;
        let total = logs.len();
        let lines = logs
            .iter()
            .skip(total.saturating_sub(requested))
            .cloned()
            .collect::<Vec<_>>();
        Ok(ApiResponse::ok(json!({ "lines": lines, "total": total })))
    }

    fn handle_process_kill(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        #[derive(serde::Deserialize)]
        struct Body {
            pid: u32,
        }
        let body: Body = request.json()?;
        if body.pid == std::process::id() {
            return Err(ApiError::new(403, "refusing to terminate the HTTP host"));
        }
        let name = process_name(body.pid).ok_or_else(|| ApiError::new(404, "process not found"))?;
        let normalized = name.to_ascii_lowercase();
        if !normalized.contains("goldeneye") && !normalized.contains("codebase-memory") {
            return Err(ApiError::new(403, "target is not a Goldeneye process"));
        }
        terminate_process(body.pid)?;
        self.log(&format!("process termination requested pid={}", body.pid));
        Ok(ApiResponse::ok(json!({ "killed": true, "pid": body.pid })))
    }

    fn query_store(&self) -> Result<QueryStore, ApiError> {
        if !self.config.database_path().is_file() {
            return Err(ApiError::new(404, "project database not found"));
        }
        Store::open_read_only(self.config.database_path()).map_err(internal)
    }

    fn allowed_root(&self) -> Result<PathBuf, ApiError> {
        let root = self
            .config
            .allowed_root()
            .unwrap_or_else(|| self.config.project_root());
        root.canonicalize()
            .map_err(|error| ApiError::new(500, format!("allowed root is unavailable: {error}")))
    }

    fn authorize_path(&self, path: &Path) -> Result<(), ApiError> {
        let allowed = self.allowed_root()?;
        if path.starts_with(allowed) {
            Ok(())
        } else {
            Err(ApiError::new(403, "path is outside the allowed root"))
        }
    }

    fn log(&self, message: &str) {
        push_log(&self.logs, message);
    }
}

impl ApiBackend for GoldeneyeBackend {
    fn handle(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/rpc") => self.handle_rpc(request),
            ("GET", "/api/layout") => self.handle_layout(request),
            ("GET", "/api/repo-info") => self.handle_repo_info(request),
            ("POST", "/api/index") => self.handle_index(request),
            ("GET", "/api/index-status") => self.handle_index_status(),
            ("GET", "/api/ui-config") => Ok(Self::handle_ui_config(request)),
            ("DELETE", "/api/project") => self.handle_delete_project(request),
            ("GET", "/api/browse") => self.handle_browse(request),
            ("GET", "/api/adr") => self.handle_adr_get(request),
            ("POST", "/api/adr") => self.handle_adr_save(request),
            ("GET", "/api/project-health") => self.handle_project_health(request),
            ("GET", "/api/processes") => Ok(self.handle_processes()),
            ("GET", "/api/logs") => self.handle_logs(request),
            ("POST", "/api/process-kill") => self.handle_process_kill(request),
            (_, path) if path == "/rpc" || path.starts_with("/api/") => {
                Err(ApiError::new(405, "method not allowed"))
            }
            _ => Err(ApiError::new(404, "not found")),
        }
    }
}

impl Drop for GoldeneyeBackend {
    fn drop(&mut self) {
        if let Ok(runtime) = self.watcher_runtime.get_mut()
            && let Some(runtime) = runtime.take()
        {
            runtime.stop();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Indexing,
    Done,
    Error,
}

impl JobStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Indexing => "indexing",
            Self::Done => "done",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
struct IndexJob {
    slot: usize,
    status: JobStatus,
    path: PathBuf,
    project_name: String,
    error: String,
}

fn layout_value(
    store: &QueryStore,
    project: &ProjectId,
    max_nodes: usize,
) -> Result<Value, ApiError> {
    let mut nodes = store.list_nodes(project).map_err(internal)?;
    let edges = store.list_edges(project).map_err(internal)?;
    let total_nodes = nodes.len();
    nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    nodes.truncate(max_nodes);

    let ids = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str().to_owned(), index))
        .collect::<BTreeMap<_, _>>();
    let mut degree = vec![0_u64; nodes.len()];
    let mut inbound_calls = vec![0_u64; nodes.len()];
    let mut layout_edges = Vec::new();
    for edge in &edges {
        let (Some(&source), Some(&target)) =
            (ids.get(edge.source.as_str()), ids.get(edge.target.as_str()))
        else {
            continue;
        };
        degree[source] += 1;
        degree[target] += 1;
        if edge.kind.as_str() == "CALLS" {
            inbound_calls[target] += 1;
        }
        layout_edges.push(json!({
            "source": source,
            "target": target,
            "type": edge.kind.as_str(),
        }));
    }

    let count = usize_to_f64(nodes.len().max(1));
    let layout_nodes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let (x, y, z) = coordinates(index, count);
            node_value(node, index, x, y, z, degree[index], inbound_calls[index])
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "nodes": layout_nodes,
        "edges": layout_edges,
        "total_nodes": total_nodes,
    }))
}

fn linked_projects_value(
    store: &QueryStore,
    source_project: &ProjectId,
    max_nodes: usize,
) -> Result<Vec<Value>, ApiError> {
    let mut source_nodes = store.list_nodes(source_project).map_err(internal)?;
    source_nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    source_nodes.truncate(max_nodes);
    let source_ids = source_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str().to_owned(), index))
        .collect::<BTreeMap<_, _>>();
    let source_qn = source_nodes
        .iter()
        .map(|node| {
            (
                node.id.as_str().to_owned(),
                node.qualified_name.as_str().to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let source_edges = store.list_edges(source_project).map_err(internal)?;
    let mut grouped = BTreeMap::<String, Vec<&GraphEdge>>::new();
    for edge in &source_edges {
        if !edge.kind.as_str().starts_with("CROSS_") {
            continue;
        }
        let Some(target_project) = edge
            .properties
            .get("target_project")
            .and_then(Value::as_str)
            .filter(|project| *project != source_project.as_str())
        else {
            continue;
        };
        grouped
            .entry(target_project.to_owned())
            .or_default()
            .push(edge);
    }

    let count = grouped.len().min(16);
    let mut linked = Vec::with_capacity(count);
    for (position, (target_name, cross)) in grouped.into_iter().take(16).enumerate() {
        let Ok(target_project) = ProjectId::new(&target_name) else {
            continue;
        };
        if store
            .get_project(&target_project)
            .map_err(internal)?
            .is_none()
        {
            continue;
        }
        let mut target_nodes = store.list_nodes(&target_project).map_err(internal)?;
        target_nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        target_nodes.truncate(max_nodes);
        let target_qn = target_nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.qualified_name.as_str().to_owned(), index))
            .collect::<BTreeMap<_, _>>();
        let target_layout = layout_value(store, &target_project, max_nodes)?;
        let object = target_layout
            .as_object()
            .expect("layout values are always JSON objects");
        let cross_edges = cross
            .into_iter()
            .filter_map(|edge| {
                let source = source_ids.get(edge.source.as_str())?;
                let qualified_name = source_qn.get(edge.target.as_str())?;
                let target = target_qn.get(qualified_name)?;
                Some(json!({
                    "source": source,
                    "target": target,
                    "type": edge.kind.as_str(),
                }))
            })
            .collect::<Vec<_>>();
        let angle = if count == 0 {
            0.0
        } else {
            2.0 * std::f64::consts::PI * usize_to_f64(position) / usize_to_f64(count)
        };
        linked.push(json!({
            "project": target_name,
            "nodes": object.get("nodes").cloned().unwrap_or_else(|| json!([])),
            "edges": object.get("edges").cloned().unwrap_or_else(|| json!([])),
            "offset": {
                "x": angle.cos() * 1_000.0,
                "y": angle.sin() * 1_000.0,
                "z": 0.0,
            },
            "cross_edges": cross_edges,
        }));
    }
    Ok(linked)
}

fn coordinates(index: usize, count: f64) -> (f64, f64, f64) {
    let position = usize_to_f64(index) + 0.5;
    let y = 1.0 - (2.0 * position / count);
    let radius = (1.0 - y * y).max(0.0).sqrt();
    let angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt()) * position;
    let scale = 24.0 * count.cbrt().max(1.0);
    (
        angle.cos() * radius * scale,
        y * scale,
        angle.sin() * radius * scale,
    )
}

fn node_value(
    node: &GraphNode,
    index: usize,
    x: f64,
    y: f64,
    z: f64,
    degree: u64,
    inbound_calls: u64,
) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), json!(index));
    object.insert("x".to_owned(), json!(x));
    object.insert("y".to_owned(), json!(y));
    object.insert("z".to_owned(), json!(z));
    object.insert("label".to_owned(), json!(node.label.as_str()));
    object.insert("name".to_owned(), json!(node.name));
    object.insert(
        "qualified_name".to_owned(),
        json!(node.qualified_name.as_str()),
    );
    let degree = f64::from(u32::try_from(degree).unwrap_or(u32::MAX));
    object.insert("size".to_owned(), json!(1.0 + (degree + 1.0).ln()));
    object.insert("color".to_owned(), json!(label_color(node.label.as_str())));
    object.insert("in_calls".to_owned(), json!(inbound_calls));
    if let Some(path) = &node.file_path {
        object.insert("file_path".to_owned(), json!(path.as_str()));
    }
    if let Some(span) = node.source_span {
        object.insert("start_line".to_owned(), json!(span.start.row + 1));
        object.insert("end_line".to_owned(), json!(span.end.row + 1));
    }
    if let Some(status) = node.properties.get("status").and_then(Value::as_str) {
        object.insert("status".to_owned(), json!(status));
    }
    Value::Object(object)
}

fn label_color(label: &str) -> &'static str {
    const COLORS: &[&str] = &[
        "#7dd3fc", "#a78bfa", "#f472b6", "#fb7185", "#fbbf24", "#4ade80", "#2dd4bf", "#60a5fa",
        "#c084fc", "#f97316", "#94a3b8", "#e879f9",
    ];
    let hash = label.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    COLORS[hash as usize % COLORS.len()]
}

fn project_id(value: &str) -> Result<ProjectId, ApiError> {
    ProjectId::new(value).map_err(|error| ApiError::new(400, error.to_string()))
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(500, error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, ApiError> {
    mutex
        .lock()
        .map_err(|_| ApiError::new(500, "shared HTTP state is poisoned"))
}

fn push_log(logs: &Mutex<VecDeque<String>>, message: &str) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() == LOG_CAPACITY {
            logs.pop_front();
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        logs.push_back(format!("{timestamp} {message}"));
    }
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

fn persistence_enabled() -> bool {
    std::env::var("CBM_PERSISTENCE").ok().is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn git_output(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn strip_remote_credentials(remote: &str) -> String {
    let Some((scheme, rest)) = remote.split_once("://") else {
        return remote.to_owned();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(authority_end);
    let safe_authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{safe_authority}{suffix}")
}

fn remote_web_base(remote: &str) -> Option<String> {
    let normalized = if let Some(rest) = remote.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        format!("https://{host}/{path}")
    } else if let Some(rest) = remote.strip_prefix("ssh://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, value)| value);
        format!("https://{rest}")
    } else if remote.starts_with("https://") {
        remote.to_owned()
    } else {
        return None;
    };
    let normalized = normalized.trim_end_matches('/').trim_end_matches(".git");
    let rest = normalized.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty()
        || path.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        || path
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
    {
        return None;
    }
    Some(format!("https://{host}/{}", encode_url_path(path)))
}

fn encode_url_path(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn format_elapsed(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(target_os = "linux")]
fn current_rss_megabytes() -> f64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<f64>()
                    .ok()
            })
        })
        .map_or(0.0, |kilobytes| kilobytes / 1_024.0)
}

#[cfg(not(target_os = "linux"))]
const fn current_rss_megabytes() -> f64 {
    0.0
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|value| value.replace('\0', " "))
        .filter(|value| !value.is_empty())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    line.strip_prefix('"')
        .and_then(|value| value.split_once('"'))
        .map(|(name, _)| name.to_owned())
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> Result<(), ApiError> {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(internal)?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| ApiError::new(500, "process termination failed"))
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> Result<(), ApiError> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .status()
        .map_err(internal)?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| ApiError::new(500, "process termination failed"))
}
