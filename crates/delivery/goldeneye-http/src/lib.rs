mod assets;
mod backend;
mod server;

pub use backend::{ApiBackend, ApiError, ApiRequest, ApiResponse, GoldeneyeBackend};
pub use server::{BoundServer, HttpError, ServerConfig, ShutdownHandle};

pub const HTTP_API_CONTRACT: &[(&str, &str)] = &[
    ("POST", "/rpc"),
    ("GET", "/api/layout"),
    ("GET", "/api/repo-info"),
    ("POST", "/api/index"),
    ("GET", "/api/index-status"),
    ("GET", "/api/ui-config"),
    ("DELETE", "/api/project"),
    ("GET", "/api/browse"),
    ("GET", "/api/adr"),
    ("POST", "/api/adr"),
    ("GET", "/api/project-health"),
    ("GET", "/api/processes"),
    ("GET", "/api/logs"),
    ("POST", "/api/process-kill"),
];

pub const UI_RPC_TOOLS: &[&str] = &["list_projects", "get_graph_schema", "get_code_snippet"];
