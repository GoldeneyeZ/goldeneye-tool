use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use goldeneye_store::Store;
use serde_json::json;

use super::{ApiError, ApiRequest, ApiResponse, GoldeneyeBackend, internal, project_id};

impl GoldeneyeBackend {
    pub(super) fn handle_repo_info(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
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

    pub(super) fn handle_browse(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let raw = request.required_query("path")?;
        let path = PathBuf::from(raw)
            .canonicalize()
            .map_err(|_| ApiError::new(400, "not a directory"))?;
        if !path.is_dir() {
            return Err(ApiError::new(400, "not a directory"));
        }
        self.authorize_path(&path)?;
        let directories = directory_names(&path)?;
        let allowed = self.allowed_root()?;
        let parent = browse_parent(&path, &allowed);
        Ok(ApiResponse::ok(json!({
            "path": path,
            "dirs": directories,
            "parent": parent,
        })))
    }

    pub(super) fn handle_adr_get(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
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

    pub(super) fn handle_adr_save(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
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

    pub(super) fn handle_project_health(
        &self,
        request: &ApiRequest,
    ) -> Result<ApiResponse, ApiError> {
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
}

fn directory_names(path: &Path) -> Result<Vec<String>, ApiError> {
    let mut directories = fs::read_dir(path)
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
    Ok(directories)
}

fn browse_parent(path: &Path, allowed: &Path) -> String {
    path.parent()
        .filter(|parent| parent.starts_with(allowed))
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
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
