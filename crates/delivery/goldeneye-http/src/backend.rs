mod indexing;
mod layout;
mod process;
mod repository;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use goldeneye_bootstrap::BootstrapRuntime;
use goldeneye_domain::ProjectId;
use goldeneye_mcp::server::Server;
use goldeneye_services::ServiceConfig;
use goldeneye_store::{QueryStore, Store};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use indexing::IndexJob;
use process::push_log;

const MAX_INDEX_JOBS: usize = 4;
const MAX_LAYOUT_NODES: usize = 250_000;
const DEFAULT_LAYOUT_NODES: usize = 5_000;
const MAX_LOG_LINES: usize = 2_000;
const LOG_CAPACITY: usize = 4_096;

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
    started: Instant,
}

impl GoldeneyeBackend {
    #[must_use]
    pub fn new(config: ServiceConfig) -> Self {
        Self::with_runtime(BootstrapRuntime::from_config(config))
    }

    #[must_use]
    pub fn with_runtime(runtime: BootstrapRuntime) -> Self {
        let config = runtime.services().config().clone();
        Self {
            rpc: Server::with_runtime(runtime),
            config,
            jobs: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::thread;
    use std::time::{Duration, Instant};

    use goldeneye_bootstrap::service_dependencies;
    use goldeneye_services::{
        ArchitectureRequest, IndexRepositoryRequest, ProjectId, ServiceConfig, ServiceErrorCode,
        Services,
    };
    use goldeneye_store::Store;

    use super::{ApiBackend, ApiRequest, GoldeneyeBackend};

    #[test]
    fn http_delete_invalidates_rpc_cache_and_untracks_the_single_registry() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("repo");
        fs::create_dir(&root).expect("repository directory");
        fs::write(root.join("lib.rs"), "fn indexed() {}\n").expect("source file");
        let config = ServiceConfig::new(temp.path().join("graph.db"), &root);
        let services = Services::new(config.clone(), service_dependencies());
        services
            .index_repository(
                &IndexRepositoryRequest::new(&root)
                    .with_name("demo")
                    .with_mode(goldeneye_services::IndexRepositoryMode::Fast),
            )
            .expect("initial index");
        drop(services);
        let backend = GoldeneyeBackend::new(config);
        let project = ProjectId::new("demo").expect("project ID");
        backend
            .rpc
            .services()
            .get_architecture(&ArchitectureRequest::new(project.clone()))
            .expect("warm RPC architecture cache");
        assert_eq!(
            backend
                .rpc
                .watcher()
                .projects()
                .expect("seeded projects")
                .len(),
            1
        );
        let request = ApiRequest {
            method: "DELETE".to_owned(),
            path: "/api/project".to_owned(),
            query: BTreeMap::from([("name".to_owned(), "demo".to_owned())]),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };

        let response = backend.handle(&request).expect("HTTP delete");

        assert_eq!(response.status, 200);
        assert!(
            backend
                .rpc
                .watcher()
                .projects()
                .expect("projects after delete")
                .is_empty()
        );
        let error = backend
            .rpc
            .services()
            .get_architecture(&ArchitectureRequest::new(project))
            .expect_err("HTTP delete invalidates RPC cache");
        assert_eq!(error.code(), ServiceErrorCode::NotFound);
    }

    #[test]
    fn detached_http_index_finishes_after_backend_drop() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("repo");
        fs::create_dir(&root).expect("repository directory");
        for index in 0..100 {
            fs::write(
                root.join(format!("file_{index}.rs")),
                format!("fn symbol_{index}() {{}}\n"),
            )
            .expect("source file");
        }
        let database = temp.path().join("graph.db");
        let backend = GoldeneyeBackend::new(ServiceConfig::new(&database, &root));
        let request = ApiRequest {
            method: "POST".to_owned(),
            path: "/api/index".to_owned(),
            query: BTreeMap::new(),
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&serde_json::json!({
                "root_path": root,
                "project_name": "detached",
                "mode": "fast"
            }))
            .expect("index request"),
        };
        assert_eq!(backend.handle(&request).expect("start index").status, 202);

        drop(backend);

        let project = ProjectId::new("detached").expect("project ID");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if Store::open_read_only(&database)
                .and_then(|store| store.get_project(&project))
                .is_ok_and(|record| record.is_some())
            {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("detached index did not persist after backend drop");
    }
}
