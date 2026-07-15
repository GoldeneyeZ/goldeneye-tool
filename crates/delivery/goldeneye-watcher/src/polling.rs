use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use super::{
    GitSnapshot, IndexDisposition, Indexer, PollReport, ProjectState, RootStatus, WatchError,
    WatchRuntime, WatchStop, Watcher, git_snapshot, lock, root_status,
};

impl<I: Indexer> Watcher<I> {
    /// Polls every due project once.
    ///
    /// Failed or busy reindexes retain their observed baseline so the next poll retries.
    ///
    /// # Errors
    ///
    /// Returns an error when shared watcher state is poisoned.
    pub fn poll_once(&self) -> Result<PollReport, WatchError> {
        let states = lock(&self.projects)?.values().cloned().collect::<Vec<_>>();
        let mut report = PollReport::default();
        for state in states {
            self.poll_project(&state, &mut report)?;
        }
        Ok(report)
    }

    /// Starts the watcher loop on a background thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the watcher thread cannot be created.
    pub fn spawn(self: &Arc<Self>) -> Result<WatchRuntime, WatchError> {
        let watcher = Arc::clone(self);
        let stop = WatchStop {
            wake: Arc::clone(&self.wake),
        };
        let join = thread::Builder::new()
            .name("goldeneye-watcher".to_owned())
            .spawn(move || watcher.run())?;
        Ok(WatchRuntime {
            stop,
            join: Some(join),
        })
    }

    fn run(&self) {
        while !self.wake.stopped.load(Ordering::Acquire) {
            let _ = self.poll_once();
            self.wake.wait(self.config.wake_interval);
        }
    }

    fn poll_project(
        &self,
        state: &Arc<Mutex<ProjectState>>,
        report: &mut PollReport,
    ) -> Result<(), WatchError> {
        let now = Instant::now();
        let state_guard = lock(state)?;
        let Some(mut state_guard) = self.check_root(state_guard, report, now)? else {
            return Ok(());
        };
        if self.initialize_snapshot(&mut state_guard, report, now) {
            return Ok(());
        }
        let Some(pending) = Self::pending_snapshot(&mut state_guard, report, now) else {
            return Ok(());
        };
        self.reindex(state, state_guard, pending, report, now)
    }

    fn check_root<'a>(
        &self,
        mut state: MutexGuard<'a, ProjectState>,
        report: &mut PollReport,
        now: Instant,
    ) -> Result<Option<MutexGuard<'a, ProjectState>>, WatchError> {
        match root_status(&state.root) {
            RootStatus::Uncertain => {
                state.missing_count = 0;
                state.first_missing = None;
                report.failed += 1;
                Ok(None)
            }
            RootStatus::Missing => {
                self.handle_missing_root(state, report, now)?;
                Ok(None)
            }
            RootStatus::Present => {
                state.missing_count = 0;
                state.first_missing = None;
                Ok(Some(state))
            }
        }
    }

    fn handle_missing_root(
        &self,
        mut state: MutexGuard<'_, ProjectState>,
        report: &mut PollReport,
        now: Instant,
    ) -> Result<(), WatchError> {
        state.missing_count += 1;
        let first = *state.first_missing.get_or_insert(now);
        let should_prune = state.missing_count >= self.config.missing_polls
            && now.duration_since(first) >= self.config.prune_grace;
        let project = state.project.clone();
        let root = state.root.clone();
        drop(state);
        if should_prune {
            match self.indexer.prune(&project, &root) {
                Ok(()) => {
                    lock(&self.projects)?.remove(&project);
                    report.pruned += 1;
                }
                Err(_) => report.failed += 1,
            }
        }
        Ok(())
    }

    fn initialize_snapshot(
        &self,
        state: &mut ProjectState,
        report: &mut PollReport,
        now: Instant,
    ) -> bool {
        if state.baseline.is_some() || state.is_git.is_some() {
            return false;
        }
        match git_snapshot(&state.root) {
            Ok(Some(snapshot)) => {
                state.file_count = snapshot.file_count;
                state.interval = self.config.poll_interval(snapshot.file_count);
                state.baseline = Some(snapshot);
                state.is_git = Some(true);
            }
            Ok(None) => state.is_git = Some(false),
            Err(()) => report.failed += 1,
        }
        state.next_poll = now + state.interval;
        true
    }

    fn pending_snapshot(
        state: &mut ProjectState,
        report: &mut PollReport,
        now: Instant,
    ) -> Option<GitSnapshot> {
        if state.is_git == Some(false) || now < state.next_poll {
            return None;
        }
        let pending = match git_snapshot(&state.root) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                state.is_git = Some(false);
                return None;
            }
            Err(()) => {
                state.next_poll = now + state.interval;
                report.failed += 1;
                return None;
            }
        };
        if state.baseline.as_ref() == Some(&pending) {
            state.next_poll = now + state.interval;
            return None;
        }
        Some(pending)
    }

    fn reindex(
        &self,
        state: &Mutex<ProjectState>,
        state_guard: MutexGuard<'_, ProjectState>,
        pending: GitSnapshot,
        report: &mut PollReport,
        now: Instant,
    ) -> Result<(), WatchError> {
        let project = state_guard.project.clone();
        let root = state_guard.root.clone();
        let interval = state_guard.interval;
        drop(state_guard);
        let result = self.indexer.index(&project, &root);
        let mut state_guard = lock(state)?;
        state_guard.next_poll = now + interval;
        match result {
            Ok(IndexDisposition::Indexed) => {
                state_guard.file_count = pending.file_count;
                state_guard.interval = self.config.poll_interval(pending.file_count);
                state_guard.baseline = Some(pending);
                report.reindexed += 1;
            }
            Ok(IndexDisposition::Busy) => report.busy += 1,
            Err(_) => report.failed += 1,
        }
        Ok(())
    }
}
