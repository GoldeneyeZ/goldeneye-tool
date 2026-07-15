use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::{
    ApiError, ApiRequest, ApiResponse, GoldeneyeBackend, LOG_CAPACITY, MAX_LOG_LINES, internal,
    lock,
};

impl GoldeneyeBackend {
    pub(super) fn handle_processes(&self) -> ApiResponse {
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

    pub(super) fn handle_logs(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
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

    pub(super) fn handle_process_kill(
        &self,
        request: &ApiRequest,
    ) -> Result<ApiResponse, ApiError> {
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
}

pub(super) fn push_log(logs: &Mutex<VecDeque<String>>, message: &str) {
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
