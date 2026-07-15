use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goldeneye_watcher::{IndexDisposition, Indexer, Watcher, WatcherConfig};

#[derive(Debug, Clone, Copy)]
enum IndexResult {
    Indexed,
    Busy,
    Error,
}

#[derive(Debug, Default)]
struct FakeState {
    index_results: VecDeque<IndexResult>,
    index_calls: usize,
    prune_calls: usize,
}

#[derive(Debug, Clone, Default)]
struct FakeIndexer {
    state: Arc<Mutex<FakeState>>,
}

impl FakeIndexer {
    fn with_results(results: impl IntoIterator<Item = IndexResult>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                index_results: results.into_iter().collect(),
                ..FakeState::default()
            })),
        }
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.state.lock().expect("fake state");
        (state.index_calls, state.prune_calls)
    }
}

impl Indexer for FakeIndexer {
    fn index(&self, _project: &str, _root: &Path) -> Result<IndexDisposition, String> {
        let mut state = self.state.lock().map_err(|_| "fake state".to_owned())?;
        state.index_calls += 1;
        match state
            .index_results
            .pop_front()
            .unwrap_or(IndexResult::Indexed)
        {
            IndexResult::Indexed => Ok(IndexDisposition::Indexed),
            IndexResult::Busy => Ok(IndexDisposition::Busy),
            IndexResult::Error => Err("index failed".to_owned()),
        }
    }

    fn prune(&self, _project: &str, _root: &Path) -> Result<(), String> {
        self.state
            .lock()
            .map_err(|_| "fake state".to_owned())?
            .prune_calls += 1;
        Ok(())
    }
}

fn immediate_config() -> WatcherConfig {
    WatcherConfig {
        poll_base: Duration::ZERO,
        poll_max: Duration::ZERO,
        file_step: 1,
        prune_grace: Duration::ZERO,
        missing_polls: 2,
        wake_interval: Duration::from_secs(30),
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Goldeneye")
        .env("GIT_AUTHOR_EMAIL", "goldeneye@example.test")
        .env("GIT_COMMITTER_NAME", "Goldeneye")
        .env("GIT_COMMITTER_EMAIL", "goldeneye@example.test")
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?}");
}

fn repository() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temporary directory");
    let root = temp.path().join("repo");
    fs::create_dir(&root).expect("repository directory");
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("lib.rs"), "fn first() {}\n").expect("seed source");
    git(&root, &["add", "lib.rs"]);
    git(&root, &["commit", "-q", "-m", "seed"]);
    (temp, root)
}

#[test]
fn first_poll_seeds_baseline_and_due_change_reindexes_once() {
    let (_temp, root) = repository();
    let indexer = FakeIndexer::default();
    let watcher = Watcher::new(immediate_config(), indexer.clone());
    watcher.watch("demo", &root).expect("watch project");

    assert_eq!(watcher.poll_once().expect("seed poll").reindexed, 0);
    assert_eq!(indexer.counts().0, 0);
    fs::write(root.join("lib.rs"), "fn changed() {}\n").expect("change source");
    assert!(watcher.touch("demo").expect("touch project"));

    assert_eq!(watcher.poll_once().expect("due poll").reindexed, 1);
    assert_eq!(indexer.counts().0, 1);
}

#[test]
fn busy_and_error_results_retry_the_same_pending_snapshot() {
    let (_temp, root) = repository();
    let indexer =
        FakeIndexer::with_results([IndexResult::Busy, IndexResult::Error, IndexResult::Indexed]);
    let watcher = Watcher::new(immediate_config(), indexer.clone());
    watcher.watch("demo", &root).expect("watch project");
    watcher.poll_once().expect("seed poll");
    fs::write(root.join("lib.rs"), "fn pending() {}\n").expect("pending source");

    assert_eq!(watcher.poll_once().expect("busy poll").busy, 1);
    assert_eq!(watcher.poll_once().expect("failed poll").failed, 1);
    assert_eq!(watcher.poll_once().expect("retry poll").reindexed, 1);
    assert_eq!(indexer.counts().0, 3);
}

#[test]
fn missing_root_requires_both_poll_count_and_prune_grace() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let missing = temp.path().join("missing");
    let guarded_indexer = FakeIndexer::default();
    let mut guarded_config = immediate_config();
    guarded_config.prune_grace = Duration::from_mins(1);
    let guarded = Watcher::new(guarded_config, guarded_indexer.clone());
    guarded.watch("guarded", &missing).expect("watch guarded");
    guarded.poll_once().expect("first missing poll");
    guarded.poll_once().expect("second missing poll");
    assert_eq!(
        guarded.projects().expect("guarded projects")[0].missing_polls,
        2
    );
    assert_eq!(guarded_indexer.counts().1, 0);

    let pruning_indexer = FakeIndexer::default();
    let pruning = Watcher::new(immediate_config(), pruning_indexer.clone());
    pruning.watch("pruned", &missing).expect("watch pruned");
    assert_eq!(pruning.poll_once().expect("first prune poll").pruned, 0);
    assert_eq!(pruning.poll_once().expect("second prune poll").pruned, 1);
    assert!(pruning.projects().expect("pruned projects").is_empty());
    assert_eq!(pruning_indexer.counts().1, 1);
}

#[test]
fn runtime_stop_wakes_and_joins_a_sleeping_watcher() {
    let watcher = Arc::new(Watcher::new(immediate_config(), FakeIndexer::default()));
    let runtime = watcher.spawn().expect("watcher runtime");
    let stop = runtime.stop_handle();
    assert!(!stop.is_stopped());
    let started = Instant::now();

    stop.stop();
    runtime.stop();

    assert!(stop.is_stopped());
    assert!(started.elapsed() < Duration::from_secs(2));
}
