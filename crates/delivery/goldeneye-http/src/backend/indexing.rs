use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use goldeneye_services::{IndexRepositoryMode, IndexRepositoryRequest};
use serde_json::json;

use super::{ApiError, ApiRequest, ApiResponse, GoldeneyeBackend, MAX_INDEX_JOBS, lock, push_log};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobStatus {
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
pub(super) struct IndexJob {
    slot: usize,
    status: JobStatus,
    path: PathBuf,
    project_name: String,
    error: String,
}

#[derive(serde::Deserialize)]
struct IndexBody {
    root_path: PathBuf,
    #[serde(default)]
    project_name: String,
    #[serde(default)]
    persistence: bool,
    #[serde(default)]
    mode: IndexRepositoryMode,
}

impl GoldeneyeBackend {
    pub(super) fn handle_index(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let body: IndexBody = request.json()?;
        let name_override = (!body.project_name.is_empty()).then(|| body.project_name.clone());
        let root = body
            .root_path
            .canonicalize()
            .map_err(|_| ApiError::new(400, "directory not found"))?;
        if !root.is_dir() {
            return Err(ApiError::new(400, "directory not found"));
        }
        self.authorize_path(&root)?;
        let slot = reserve_index_job(&self.jobs, &root, body.project_name)?;
        self.log(&format!("index[{slot}] started {}", root.display()));
        self.spawn_index_thread(slot, &root, name_override, body.mode, body.persistence)?;
        Ok(ApiResponse::new(
            202,
            json!({ "status": "indexing", "slot": slot, "path": root }),
        ))
    }

    fn spawn_index_thread(
        &self,
        slot: usize,
        root: &Path,
        name_override: Option<String>,
        mode: IndexRepositoryMode,
        persistence: bool,
    ) -> Result<(), ApiError> {
        let jobs = Arc::clone(&self.jobs);
        let logs = Arc::clone(&self.logs);
        let watcher = Arc::clone(self.rpc.watcher());
        let services = self.rpc.services().clone();
        let thread_root = root.to_path_buf();
        thread::Builder::new()
            .name(format!("goldeneye-index-{slot}"))
            .spawn(move || {
                let result = services.index_repository(&IndexRepositoryRequest {
                    repo_path: thread_root.clone(),
                    name: name_override,
                    mode,
                    persistence: persistence || persistence_enabled(),
                });
                let (status, error) = match &result {
                    Ok(indexed) => {
                        let _ = watcher.watch(&indexed.project, &thread_root);
                        (JobStatus::Done, String::new())
                    }
                    Err(error) => (JobStatus::Error, error.to_string()),
                };
                update_index_job(&jobs, slot, status, &error);
                push_log(&logs, &index_message(slot, status, &thread_root, error));
            })
            .map_err(|error| ApiError::new(500, format!("thread creation failed: {error}")))?;
        Ok(())
    }

    pub(super) fn handle_index_status(&self) -> Result<ApiResponse, ApiError> {
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

    pub(super) fn handle_delete_project(
        &self,
        request: &ApiRequest,
    ) -> Result<ApiResponse, ApiError> {
        let project = super::project_id(request.required_query("name")?)?;
        if !self
            .rpc
            .services()
            .delete_project(&project)
            .map_err(super::internal)?
        {
            return Err(ApiError::new(404, "project not found"));
        }
        let _ = self.rpc.watcher().unwatch(project.as_str());
        self.log(&format!("project deleted {}", project.as_str()));
        Ok(ApiResponse::ok(json!({ "deleted": true })))
    }
}

fn reserve_index_job(
    jobs: &Mutex<Vec<IndexJob>>,
    root: &PathBuf,
    project_name: String,
) -> Result<usize, ApiError> {
    let mut jobs = lock(jobs)?;
    if jobs
        .iter()
        .any(|job| job.status == JobStatus::Indexing && job.path == *root)
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
        project_name,
        error: String::new(),
    };
    if slot == jobs.len() {
        jobs.push(job);
    } else {
        jobs[slot] = job;
    }
    drop(jobs);
    Ok(slot)
}

fn update_index_job(jobs: &Mutex<Vec<IndexJob>>, slot: usize, status: JobStatus, error: &String) {
    if let Ok(mut jobs) = jobs.lock()
        && let Some(job) = jobs.get_mut(slot)
    {
        job.status = status;
        job.error.clone_from(error);
    }
}

fn index_message(slot: usize, status: JobStatus, thread_root: &Path, error: String) -> String {
    format!(
        "index[{slot}] {} {}",
        status.as_str(),
        if error.is_empty() {
            thread_root.display().to_string()
        } else {
            error
        }
    )
}

fn persistence_enabled() -> bool {
    std::env::var("CBM_PERSISTENCE").ok().is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
