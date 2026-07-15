use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use thiserror::Error;

mod polling;

pub const DEFAULT_POLL_BASE: Duration = Duration::from_secs(5);
pub const DEFAULT_POLL_MAX: Duration = Duration::from_mins(1);
pub const DEFAULT_FILE_STEP: usize = 500;
pub const DEFAULT_PRUNE_GRACE: Duration = Duration::from_mins(10);
pub const DEFAULT_MISSING_POLLS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherConfig {
    pub poll_base: Duration,
    pub poll_max: Duration,
    pub file_step: usize,
    pub prune_grace: Duration,
    pub missing_polls: u32,
    pub wake_interval: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            poll_base: DEFAULT_POLL_BASE,
            poll_max: DEFAULT_POLL_MAX,
            file_step: DEFAULT_FILE_STEP,
            prune_grace: prune_grace_from_env(),
            missing_polls: DEFAULT_MISSING_POLLS,
            wake_interval: Duration::from_millis(250),
        }
    }
}

impl WatcherConfig {
    #[must_use]
    pub fn poll_interval(&self, file_count: usize) -> Duration {
        let steps = file_count / self.file_step.max(1);
        self.poll_base
            .saturating_add(Duration::from_secs(steps as u64))
            .min(self.poll_max)
    }
}

pub trait Indexer: Send + Sync + 'static {
    /// # Errors
    ///
    /// Returns a stable message when indexing cannot complete.
    fn index(&self, project: &str, root: &Path) -> Result<IndexDisposition, String>;

    /// # Errors
    ///
    /// Returns a stable message when pruning cannot complete.
    fn prune(&self, _project: &str, _root: &Path) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexDisposition {
    Indexed,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedProject {
    pub project: String,
    pub root: PathBuf,
    pub is_git: Option<bool>,
    pub file_count: usize,
    pub interval: Duration,
    pub missing_polls: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollReport {
    pub reindexed: usize,
    pub busy: usize,
    pub failed: usize,
    pub pruned: usize,
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("watcher state is poisoned")]
    Poisoned,
    #[error("watcher thread creation failed: {0}")]
    Thread(#[from] io::Error),
}

pub struct Watcher<I> {
    config: WatcherConfig,
    indexer: Arc<I>,
    projects: Mutex<BTreeMap<String, Arc<Mutex<ProjectState>>>>,
    wake: Arc<WakeState>,
}

impl<I: Indexer> Watcher<I> {
    #[must_use]
    pub fn new(config: WatcherConfig, indexer: I) -> Self {
        Self {
            config,
            indexer: Arc::new(indexer),
            projects: Mutex::new(BTreeMap::new()),
            wake: Arc::new(WakeState::default()),
        }
    }

    /// Adds or replaces a watched project.
    ///
    /// # Errors
    ///
    /// Returns an error when shared watcher state is poisoned.
    pub fn watch(
        &self,
        project: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Result<(), WatchError> {
        let project = project.into();
        let state = ProjectState::new(project.clone(), root.into(), self.config.poll_base);
        lock(&self.projects)?.insert(project, Arc::new(Mutex::new(state)));
        self.wake.notify();
        Ok(())
    }

    /// Removes a watched project.
    ///
    /// # Errors
    ///
    /// Returns an error when shared watcher state is poisoned.
    pub fn unwatch(&self, project: &str) -> Result<bool, WatchError> {
        let removed = lock(&self.projects)?.remove(project).is_some();
        self.wake.notify();
        Ok(removed)
    }

    /// Schedules a project for immediate polling.
    ///
    /// # Errors
    ///
    /// Returns an error when shared watcher state is poisoned.
    pub fn touch(&self, project: &str) -> Result<bool, WatchError> {
        let state = lock(&self.projects)?.get(project).cloned();
        let Some(state) = state else {
            return Ok(false);
        };
        lock(&state)?.next_poll = Instant::now();
        self.wake.notify();
        Ok(true)
    }

    /// Returns compact watcher state for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error when shared watcher state is poisoned.
    pub fn projects(&self) -> Result<Vec<WatchedProject>, WatchError> {
        let states = lock(&self.projects)?.values().cloned().collect::<Vec<_>>();
        states
            .into_iter()
            .map(|state| {
                let state = lock(&state)?;
                Ok(WatchedProject {
                    project: state.project.clone(),
                    root: state.root.clone(),
                    is_git: state.is_git,
                    file_count: state.file_count,
                    interval: state.interval,
                    missing_polls: state.missing_count,
                })
            })
            .collect()
    }

    /// Returns whether the shared watcher runtime has been asked to stop.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.wake.stopped.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitSnapshot {
    head: String,
    dirty_hash: [u8; 32],
    file_count: usize,
}

#[derive(Debug)]
struct ProjectState {
    project: String,
    root: PathBuf,
    baseline: Option<GitSnapshot>,
    is_git: Option<bool>,
    file_count: usize,
    interval: Duration,
    next_poll: Instant,
    missing_count: u32,
    first_missing: Option<Instant>,
}

impl ProjectState {
    fn new(project: String, root: PathBuf, interval: Duration) -> Self {
        Self {
            project,
            root,
            baseline: None,
            is_git: None,
            file_count: 0,
            interval,
            next_poll: Instant::now(),
            missing_count: 0,
            first_missing: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootStatus {
    Present,
    Missing,
    Uncertain,
}

fn root_status(root: &Path) -> RootStatus {
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => RootStatus::Present,
        Ok(_) => RootStatus::Missing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => RootStatus::Missing,
        Err(_) => RootStatus::Uncertain,
    }
}

fn git_snapshot(root: &Path) -> Result<Option<GitSnapshot>, ()> {
    let inside = git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if String::from_utf8_lossy(&inside).trim() != "true" {
        return Ok(None);
    }
    let head = String::from_utf8_lossy(&git(root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_owned();
    let dirty = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )?;
    let files = git(root, &["ls-files", "-co", "--exclude-standard", "-z"])?;
    let dirty_hash: [u8; 32] = Sha256::digest(dirty).into();
    let file_count = files
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .count();
    Ok(Some(GitSnapshot {
        head,
        dirty_hash,
        file_count,
    }))
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, ()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|_| ())?;
    output.status.success().then_some(output.stdout).ok_or(())
}

#[derive(Debug, Default)]
struct WakeState {
    stopped: AtomicBool,
    signal: Mutex<()>,
    condvar: Condvar,
}

impl WakeState {
    fn notify(&self) {
        self.condvar.notify_all();
    }

    fn wait(&self, duration: Duration) {
        if let Ok(signal) = self.signal.lock() {
            let _ = self.condvar.wait_timeout(signal, duration);
        }
    }
}

#[derive(Debug, Clone)]
pub struct WatchStop {
    wake: Arc<WakeState>,
}

impl WatchStop {
    pub fn stop(&self) {
        self.wake.stopped.store(true, Ordering::Release);
        self.wake.notify();
    }

    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.wake.stopped.load(Ordering::Acquire)
    }
}

pub struct WatchRuntime {
    stop: WatchStop,
    join: Option<JoinHandle<()>>,
}

impl WatchRuntime {
    #[must_use]
    pub fn stop_handle(&self) -> WatchStop {
        self.stop.clone()
    }

    pub fn stop(mut self) {
        self.stop.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for WatchRuntime {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, WatchError> {
    mutex.lock().map_err(|_| WatchError::Poisoned)
}

fn prune_grace_from_env() -> Duration {
    std::env::var("CBM_WATCHER_PRUNE_GRACE_S")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(DEFAULT_PRUNE_GRACE, Duration::from_secs)
}
