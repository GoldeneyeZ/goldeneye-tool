use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::Path,
};

use super::{
    Cancellation, GitCoChange, GitError, GitFileHistory, GitHistory, GitLimits, MAX_COUPLINGS,
    MAX_FILE_HISTORY, MAX_FILES_PER_COMMIT, MAX_HISTORY_COMMITS, MIN_CO_CHANGES,
    MIN_COUPLING_SCORE, process::run_git,
};

#[derive(Default)]
struct Commit {
    timestamp: i64,
    files: BTreeSet<String>,
    too_large: bool,
}

#[derive(Default)]
struct HistoryAccumulator {
    temporal: BTreeMap<String, (u64, i64)>,
    pairs: BTreeMap<(String, String), (u64, i64)>,
    current: Option<Commit>,
}

/// Collects bounded file-change and co-change history for a repository.
///
/// # Errors
///
/// Returns a Git execution error when history collection is cancelled, times out,
/// exceeds configured output limits, or cannot start/read Git.
pub fn collect_history(
    root: &Path,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<GitHistory, GitError> {
    let max_count = format!("--max-count={MAX_HISTORY_COMMITS}");
    let args = [
        OsString::from("log"),
        OsString::from("--name-only"),
        OsString::from("--pretty=format:COMMIT:%H:%ct"),
        OsString::from("--since=1 year ago"),
        OsString::from(max_count),
        OsString::from("--"),
    ];
    let capture = run_git(root, &args, cancellation, limits)?;
    if !capture.status.success() {
        return Ok(GitHistory::default());
    }
    Ok(parse_history_log(&String::from_utf8_lossy(&capture.stdout)))
}

#[must_use]
pub fn parse_history_log(output: &str) -> GitHistory {
    let mut history = HistoryAccumulator::default();
    for raw_line in output.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(header) = line.strip_prefix("COMMIT:") {
            history.start_commit(commit_timestamp(header));
        } else if !line.is_empty() && is_trackable_file(line) {
            history.add_file(line);
        }
    }
    history.finish()
}

impl HistoryAccumulator {
    fn start_commit(&mut self, timestamp: i64) {
        self.apply_current();
        self.current = Some(Commit {
            timestamp,
            ..Commit::default()
        });
    }

    fn add_file(&mut self, path: &str) {
        let Some(commit) = self.current.as_mut() else {
            return;
        };
        if commit.files.len() < MAX_FILES_PER_COMMIT + 1 {
            commit.files.insert(path.to_owned());
        }
        if commit.files.len() > MAX_FILES_PER_COMMIT {
            commit.too_large = true;
        }
    }

    fn apply_current(&mut self) {
        if let Some(commit) = self.current.take() {
            apply_commit(&commit, &mut self.temporal, &mut self.pairs);
        }
    }

    fn finish(mut self) -> GitHistory {
        self.apply_current();
        history_from_maps(&self.temporal, self.pairs)
    }
}

fn commit_timestamp(header: &str) -> i64 {
    header
        .rsplit_once(':')
        .and_then(|(_, value)| value.parse::<i64>().ok())
        .unwrap_or(0)
}

fn apply_commit(
    commit: &Commit,
    temporal: &mut BTreeMap<String, (u64, i64)>,
    pairs: &mut BTreeMap<(String, String), (u64, i64)>,
) {
    if commit.too_large || commit.files.is_empty() {
        return;
    }
    let files = commit.files.iter().collect::<Vec<_>>();
    for file in &files {
        let entry = temporal.entry((*file).clone()).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.max(commit.timestamp);
    }
    for left in 0..files.len() {
        for right in (left + 1)..files.len() {
            let key = (files[left].clone(), files[right].clone());
            let entry = pairs.entry(key).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.max(commit.timestamp);
        }
    }
}

fn history_from_maps(
    temporal: &BTreeMap<String, (u64, i64)>,
    pairs: BTreeMap<(String, String), (u64, i64)>,
) -> GitHistory {
    let files = temporal
        .iter()
        .take(MAX_FILE_HISTORY)
        .map(|(path, (change_count, last_modified))| GitFileHistory {
            path: path.clone(),
            change_count: *change_count,
            last_modified: *last_modified,
        })
        .collect();
    let couplings = couplings(temporal, pairs);
    GitHistory { files, couplings }
}

fn couplings(
    temporal: &BTreeMap<String, (u64, i64)>,
    pairs: BTreeMap<(String, String), (u64, i64)>,
) -> Vec<GitCoChange> {
    pairs
        .into_iter()
        .filter_map(|((file_a, file_b), (co_changes, last_co_change))| {
            if co_changes < MIN_CO_CHANGES {
                return None;
            }
            let left = temporal.get(&file_a)?.0;
            let right = temporal.get(&file_b)?.0;
            let denominator = left.min(right);
            let coupling_score = f64::from(u32::try_from(co_changes).ok()?)
                / f64::from(u32::try_from(denominator).ok()?);
            (coupling_score >= MIN_COUPLING_SCORE).then_some(GitCoChange {
                file_a,
                file_b,
                co_changes,
                coupling_score,
                last_co_change,
            })
        })
        .take(MAX_COUPLINGS)
        .collect()
}

#[must_use]
pub fn is_trackable_file(path: &str) -> bool {
    const PREFIXES: [&str; 5] = [
        ".git/",
        "node_modules/",
        "vendor/",
        "__pycache__/",
        ".cache/",
    ];
    const LOCK_FILES: [&str; 8] = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Cargo.lock",
        "poetry.lock",
        "composer.lock",
        "Gemfile.lock",
        "Pipfile.lock",
    ];
    const SUFFIXES: [&str; 11] = [
        ".lock", ".sum", ".min.js", ".min.css", ".map", ".wasm", ".png", ".jpg", ".gif", ".ico",
        ".svg",
    ];
    if path.is_empty() || PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return false;
    }
    let basename = path.rsplit('/').next().unwrap_or(path);
    !LOCK_FILES.contains(&basename) && !SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
}
