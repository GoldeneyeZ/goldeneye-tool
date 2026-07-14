#![forbid(unsafe_code)]

//! Bounded, shell-free Git context, history, and change discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_HISTORY_COMMITS: usize = 10_000;
pub const MAX_FILES_PER_COMMIT: usize = 20;
pub const MAX_COUPLINGS: usize = 8_192;
pub const MAX_FILE_HISTORY: usize = 16_384;
pub const MIN_CO_CHANGES: u64 = 3;
pub const MIN_COUPLING_SCORE: f64 = 0.3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLimits {
    pub max_output_bytes: usize,
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for GitLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 32 * 1024 * 1024,
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(10),
        }
    }
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

impl<F> Cancellation for F
where
    F: Fn() -> bool + Send + Sync,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

#[derive(Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("Git operation was cancelled")]
    Cancelled,
    #[error("Git operation timed out after {0:?}")]
    TimedOut(Duration),
    #[error("Git output exceeded the {limit}-byte limit")]
    OutputLimit { limit: usize },
    #[error("cannot start Git: {0}")]
    Spawn(#[source] io::Error),
    #[error("cannot read Git output: {0}")]
    Output(#[source] io::Error),
    #[error("base_branch contains invalid characters")]
    InvalidReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitContext {
    pub input_path: String,
    pub is_git: bool,
    pub is_worktree: bool,
    pub is_detached: bool,
    pub root_exists: bool,
    pub worktree_root: String,
    pub git_dir: String,
    pub git_common_dir: String,
    pub canonical_root: String,
    pub branch: String,
    pub branch_slug: String,
    pub head_sha: String,
    pub base_sha: String,
}

impl GitContext {
    fn empty(path: &Path, root_exists: bool) -> Self {
        Self {
            input_path: path.to_string_lossy().into_owned(),
            is_git: false,
            is_worktree: false,
            is_detached: false,
            root_exists,
            worktree_root: String::new(),
            git_dir: String::new(),
            git_common_dir: String::new(),
            canonical_root: String::new(),
            branch: String::new(),
            branch_slug: String::new(),
            head_sha: String::new(),
            base_sha: String::new(),
        }
    }

    #[must_use]
    pub fn branch_qualified_name(&self, project: &str) -> String {
        let project = if project.is_empty() {
            "project"
        } else {
            project
        };
        let slug = if self.is_detached {
            "detached"
        } else if self.is_git && !self.branch_slug.is_empty() {
            &self.branch_slug
        } else {
            "working-tree"
        };
        format!("{project}.__branch__.{slug}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFileHistory {
    pub path: String,
    pub change_count: u64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitCoChange {
    pub file_a: String,
    pub file_b: String,
    pub co_changes: u64,
    pub coupling_score: f64,
    pub last_co_change: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GitHistory {
    pub files: Vec<GitFileHistory>,
    pub couplings: Vec<GitCoChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedHunk {
    pub path: String,
    pub start_line: u64,
    pub end_line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectChangesOptions {
    pub base_branch: String,
    pub since: Option<String>,
}

impl Default for DetectChangesOptions {
    fn default() -> Self {
        Self {
            base_branch: "main".to_owned(),
            since: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFailure {
    pub status: i32,
    pub reference: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedChanges {
    pub files: Vec<String>,
    pub failure: Option<GitFailure>,
}

#[derive(Debug)]
struct Capture {
    status: ExitStatus,
    stdout: Vec<u8>,
}

pub fn resolve_context(
    path: &Path,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<GitContext, GitError> {
    let root_exists = path.exists();
    let mut context = GitContext::empty(path, root_exists);
    if !root_exists {
        return Ok(context);
    }

    let Some(worktree_root) = capture_one(
        path,
        &["rev-parse", "--show-toplevel"],
        cancellation,
        limits,
    )?
    else {
        return Ok(context);
    };
    context.is_git = true;
    context.worktree_root = worktree_root;
    context.git_dir =
        capture_one(path, &["rev-parse", "--git-dir"], cancellation, limits)?.unwrap_or_default();
    context.git_common_dir = capture_one(
        path,
        &["rev-parse", "--git-common-dir"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    context.head_sha = capture_one(
        path,
        &["rev-parse", "--verify", "HEAD"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    context.branch = capture_one(
        path,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        cancellation,
        limits,
    )?
    .unwrap_or_else(|| {
        context.is_detached = true;
        "DETACHED".to_owned()
    });
    context.is_worktree = !context.git_dir.is_empty()
        && !context.git_common_dir.is_empty()
        && normalize_git_path(&context.git_dir) != normalize_git_path(&context.git_common_dir);
    let absolute_common = capture_one(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        cancellation,
        limits,
    )?;
    context.canonical_root = canonical_repository_root(
        path,
        Path::new(&context.worktree_root),
        &context.git_common_dir,
        absolute_common.as_deref(),
    );
    context.branch_slug = slug_from_branch(&context.branch, context.is_detached);
    context.base_sha = capture_one(
        path,
        &["merge-base", "HEAD", "@{upstream}"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    Ok(context)
}

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
    #[derive(Default)]
    struct Commit {
        timestamp: i64,
        files: BTreeSet<String>,
        too_large: bool,
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

    let mut temporal = BTreeMap::<String, (u64, i64)>::new();
    let mut pairs = BTreeMap::<(String, String), (u64, i64)>::new();
    let mut current: Option<Commit> = None;
    for raw_line in output.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(header) = line.strip_prefix("COMMIT:") {
            if let Some(commit) = current.take() {
                apply_commit(&commit, &mut temporal, &mut pairs);
            }
            let timestamp = header
                .rsplit_once(':')
                .and_then(|(_, value)| value.parse::<i64>().ok())
                .unwrap_or(0);
            current = Some(Commit {
                timestamp,
                ..Commit::default()
            });
        } else if !line.is_empty() && is_trackable_file(line) {
            if let Some(commit) = current.as_mut() {
                if commit.files.len() < MAX_FILES_PER_COMMIT + 1 {
                    commit.files.insert(line.to_owned());
                }
                if commit.files.len() > MAX_FILES_PER_COMMIT {
                    commit.too_large = true;
                }
            }
        }
    }
    if let Some(commit) = current {
        apply_commit(&commit, &mut temporal, &mut pairs);
    }

    let files = temporal
        .iter()
        .take(MAX_FILE_HISTORY)
        .map(|(path, (change_count, last_modified))| GitFileHistory {
            path: path.clone(),
            change_count: *change_count,
            last_modified: *last_modified,
        })
        .collect::<Vec<_>>();
    let couplings = pairs
        .into_iter()
        .filter_map(|((file_a, file_b), (co_changes, last_co_change))| {
            if co_changes < MIN_CO_CHANGES {
                return None;
            }
            let left = temporal.get(&file_a)?.0;
            let right = temporal.get(&file_b)?.0;
            let denominator = left.min(right);
            let coupling_score = co_changes as f64 / denominator as f64;
            (coupling_score >= MIN_COUPLING_SCORE).then_some(GitCoChange {
                file_a,
                file_b,
                co_changes,
                coupling_score,
                last_co_change,
            })
        })
        .take(MAX_COUPLINGS)
        .collect();
    GitHistory { files, couplings }
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

pub fn detect_changes(
    root: &Path,
    options: &DetectChangesOptions,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<DetectedChanges, GitError> {
    let reference = options
        .since
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&options.base_branch);
    validate_reference(reference)?;
    let range = format!("{reference}...HEAD");
    let mut files = BTreeSet::new();
    let base_args = [
        OsString::from("diff"),
        OsString::from("--name-only"),
        OsString::from("-z"),
        OsString::from("--find-renames"),
        OsString::from(&range),
        OsString::from("--"),
    ];
    let base = run_git(root, &base_args, cancellation, limits)?;
    add_nul_paths(&base.stdout, &mut files);

    let local_args = [
        OsString::from("diff"),
        OsString::from("--name-only"),
        OsString::from("-z"),
        OsString::from("--find-renames"),
        OsString::from("--"),
    ];
    let local = run_git(root, &local_args, cancellation, limits)?;
    add_nul_paths(&local.stdout, &mut files);

    let status_args = [
        OsString::from("--no-optional-locks"),
        OsString::from("status"),
        OsString::from("--porcelain=v1"),
        OsString::from("-z"),
        OsString::from("--untracked-files=normal"),
        OsString::from("--"),
    ];
    let status = run_git(root, &status_args, cancellation, limits)?;
    add_status_paths(&status.stdout, &mut files);

    let failure = (!base.status.success() && files.is_empty()).then(|| GitFailure {
        status: status_code(&base.status),
        reference: reference.to_owned(),
    });
    Ok(DetectedChanges {
        files: files.into_iter().collect(),
        failure,
    })
}

#[must_use]
pub fn parse_name_status(output: &str, max: usize) -> Vec<ChangedFile> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim_end_matches('\r').split('\t');
            let status = fields.next()?.chars().next()?;
            let first = fields.next()?;
            let second = fields.next();
            let (path, old_path) = if status == 'R' {
                (second.unwrap_or(first), Some(first.to_owned()))
            } else {
                (first, None)
            };
            is_trackable_file(path).then(|| ChangedFile {
                status,
                path: path.to_owned(),
                old_path,
            })
        })
        .take(max)
        .collect()
}

#[must_use]
pub fn parse_hunks(output: &str, max: usize) -> Vec<ChangedHunk> {
    let mut current_file = None::<String>;
    let mut hunks = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(path.trim_end_matches('\r').to_owned());
            continue;
        }
        if !line.starts_with("@@") || hunks.len() >= max {
            continue;
        }
        let Some(path) = current_file
            .as_deref()
            .filter(|path| is_trackable_file(path))
        else {
            continue;
        };
        let Some(plus) = line.find('+') else {
            continue;
        };
        let range = line[(plus + 1)..]
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let (start, count) = parse_range(range);
        if start == 0 {
            continue;
        }
        let end_line = start.saturating_add(count.saturating_sub(1)).max(start);
        hunks.push(ChangedHunk {
            path: path.to_owned(),
            start_line: start,
            end_line,
        });
    }
    hunks
}

#[must_use]
pub fn parse_range(value: &str) -> (u64, u64) {
    value.split_once(',').map_or_else(
        || (value.parse().unwrap_or(0), 1),
        |(start, count)| (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
    )
}

/// Validates the selected revision before any project or subprocess work.
///
/// This preserves the upstream option-injection rejection while command execution itself uses
/// argument vectors and never interpolates a shell command.
///
/// # Errors
///
/// Returns [`GitError::InvalidReference`] for an option-like or shell-metacharacter value.
pub fn validate_reference(reference: &str) -> Result<(), GitError> {
    if reference.starts_with('-')
        || reference.contains([
            '\'', '"', ';', '|', '&', '$', '`', '<', '>', '\n', '\r', '\0',
        ])
        || (!cfg!(windows) && reference.contains('\\'))
    {
        return Err(GitError::InvalidReference);
    }
    Ok(())
}

fn add_nul_paths(output: &[u8], files: &mut BTreeSet<String>) {
    for raw in output.split(|byte| *byte == 0) {
        let path = String::from_utf8_lossy(raw);
        let path = path.trim_matches(['\r', '\n']);
        if !path.is_empty() {
            files.insert(path.to_owned());
        }
    }
}

fn add_status_paths(output: &[u8], files: &mut BTreeSet<String>) {
    let entries = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut index = 0;
    while index < entries.len() {
        let entry = entries[index];
        index += 1;
        if entry.len() < 4 {
            continue;
        }
        let status = &entry[..2];
        let path = String::from_utf8_lossy(&entry[3..]);
        if !path.is_empty() {
            files.insert(path.into_owned());
        }
        if status.iter().any(|byte| matches!(byte, b'R' | b'C')) {
            index = index.saturating_add(1);
        }
    }
}

fn capture_one(
    root: &Path,
    args: &[&str],
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Option<String>, GitError> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let capture = run_git(root, &args, cancellation, limits)?;
    if !capture.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&capture.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    Ok((!value.is_empty()).then_some(value))
}

fn run_git(
    root: &Path,
    args: &[OsString],
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Capture, GitError> {
    if cancellation.is_cancelled() {
        return Err(GitError::Cancelled);
    }
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(GitError::Spawn)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let limit = limits.max_output_bytes;
    let stdout_thread = thread::spawn(move || read_bounded(stdout, limit));
    let stderr_thread = thread::spawn(move || read_bounded(stderr, limit));
    let started = Instant::now();
    let status = loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            join_reader(stdout_thread)?;
            join_reader(stderr_thread)?;
            return Err(GitError::Cancelled);
        }
        if started.elapsed() >= limits.timeout {
            let _ = child.kill();
            let _ = child.wait();
            join_reader(stdout_thread)?;
            join_reader(stderr_thread)?;
            return Err(GitError::TimedOut(limits.timeout));
        }
        match child.try_wait().map_err(GitError::Output)? {
            Some(status) => break status,
            None => thread::sleep(limits.poll_interval),
        }
    };
    let (stdout, stdout_truncated) = join_reader(stdout_thread)?;
    let (stderr, stderr_truncated) = join_reader(stderr_thread)?;
    if stdout_truncated || stderr_truncated {
        return Err(GitError::OutputLimit { limit });
    }
    let _ = stderr;
    Ok(Capture { status, stdout })
}

fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let take_limit = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader.by_ref().take(take_limit).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
    }
    Ok((bytes, truncated))
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool), GitError> {
    handle
        .join()
        .map_err(|_| GitError::Output(io::Error::other("Git output reader panicked")))?
        .map_err(GitError::Output)
}

fn status_code(status: &ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

fn normalize_git_path(value: &str) -> String {
    value.replace('\\', "/").trim_end_matches('/').to_owned()
}

fn canonical_repository_root(
    input: &Path,
    worktree_root: &Path,
    common_dir: &str,
    absolute_common: Option<&str>,
) -> String {
    let common = absolute_common
        .filter(|value| Path::new(value).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let value = Path::new(common_dir);
            if value.is_absolute() {
                value.to_path_buf()
            } else if common_dir.is_empty() {
                worktree_root.to_path_buf()
            } else {
                input.join(value)
            }
        });
    let common = common.canonicalize().unwrap_or(common);
    let root = if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().unwrap_or(&common).to_path_buf()
    } else {
        common
    };
    root.to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_owned()
}

fn slug_from_branch(branch: &str, detached: bool) -> String {
    let fallback = if detached { "detached" } else { "working-tree" };
    let source = if detached || branch.is_empty() {
        fallback
    } else {
        branch
    };
    let mut slug = String::new();
    let mut in_dash = false;
    for character in source.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            if slug.is_empty() && character == '-' {
                in_dash = true;
                continue;
            }
            slug.push(character);
            in_dash = false;
        } else if !slug.is_empty() && !in_dash {
            slug.push('-');
            in_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        fallback.to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DetectChangesOptions, GitError, GitLimits, NeverCancel, collect_history, detect_changes,
        is_trackable_file, parse_history_log, parse_hunks, parse_name_status, parse_range,
        resolve_context,
    };
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Goldeneye")
            .env("GIT_AUTHOR_EMAIL", "goldeneye@example.test")
            .env("GIT_COMMITTER_NAME", "Goldeneye")
            .env("GIT_COMMITTER_EMAIL", "goldeneye@example.test")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }

    fn repository() -> TempDir {
        let directory = tempfile::tempdir().expect("temp repo");
        git(directory.path(), &["init", "-q", "-b", "main"]);
        fs::write(directory.path().join("base.rs"), "fn base() {}\n").expect("base");
        git(directory.path(), &["add", "base.rs"]);
        git(directory.path(), &["commit", "-q", "-m", "base"]);
        directory
    }

    #[test]
    fn history_parity_filters_scores_and_skips_large_commits() {
        let mut log = String::new();
        for timestamp in 1..=3 {
            log.push_str(&format!(
                "COMMIT:{timestamp}:{timestamp}\na.rs\nb.rs\nCargo.lock\n"
            ));
        }
        log.push_str("COMMIT:large:9\n");
        for index in 0..=20 {
            log.push_str(&format!("large/{index}.rs\n"));
        }
        let history = parse_history_log(&log);
        assert_eq!(history.files.len(), 2);
        assert_eq!(history.couplings.len(), 1);
        assert_eq!(history.couplings[0].co_changes, 3);
        assert!((history.couplings[0].coupling_score - 1.0).abs() < f64::EPSILON);
        assert!(is_trackable_file("src/lib.rs"));
        assert!(!is_trackable_file("node_modules/pkg/index.js"));
        assert!(!is_trackable_file("image.png"));
    }

    #[test]
    fn parsers_preserve_rename_destination_and_new_side_hunks() {
        let changes = parse_name_status("M\tsrc/a.rs\nR100\tsrc/old.rs\tsrc/new.rs\n", 10);
        assert_eq!(changes[1].path, "src/new.rs");
        assert_eq!(changes[1].old_path.as_deref(), Some("src/old.rs"));
        let hunks = parse_hunks("+++ b/src/a.rs\n@@ -1 +10,0 @@\n", 10);
        assert_eq!((hunks[0].start_line, hunks[0].end_line), (10, 10));
        assert_eq!(parse_range("2147483647,2"), (2_147_483_647, 2));
    }

    #[test]
    fn context_and_changes_cover_dirty_untracked_rename_and_no_git() {
        let repo = repository();
        let context =
            resolve_context(repo.path(), &NeverCancel, &GitLimits::default()).expect("git context");
        assert!(context.is_git);
        assert_eq!(context.branch, "main");
        assert_eq!(
            context.branch_qualified_name("demo"),
            "demo.__branch__.main"
        );

        fs::write(repo.path().join("base.rs"), "fn changed() {}\n").expect("dirty");
        fs::write(repo.path().join("new.rs"), "fn new() {}\n").expect("untracked");
        git(repo.path(), &["mv", "base.rs", "renamed.rs"]);
        let changes = detect_changes(
            repo.path(),
            &DetectChangesOptions::default(),
            &NeverCancel,
            &GitLimits::default(),
        )
        .expect("changes");
        assert!(changes.files.contains(&"new.rs".to_owned()));
        assert!(changes.files.contains(&"renamed.rs".to_owned()));

        let plain = tempfile::tempdir().expect("plain directory");
        let context = resolve_context(plain.path(), &NeverCancel, &GitLimits::default())
            .expect("non-git context");
        assert!(!context.is_git);
        let changes = detect_changes(
            plain.path(),
            &DetectChangesOptions::default(),
            &NeverCancel,
            &GitLimits::default(),
        )
        .expect("non-git changes");
        assert!(changes.files.is_empty());
        assert!(changes.failure.is_some());
    }

    #[test]
    fn history_uses_real_git_and_cancellation_is_observed_before_spawn() {
        let repo = repository();
        let history =
            collect_history(repo.path(), &NeverCancel, &GitLimits::default()).expect("history");
        assert_eq!(history.files[0].path, "base.rs");
        let cancelled = || true;
        let error =
            resolve_context(repo.path(), &cancelled, &GitLimits::default()).expect_err("cancelled");
        assert!(matches!(error, GitError::Cancelled));
    }

    #[test]
    fn option_like_and_shell_metacharacter_references_are_rejected() {
        let repo = repository();
        for reference in ["--output=/tmp/pwn", "main;touch pwn"] {
            let error = detect_changes(
                repo.path(),
                &DetectChangesOptions {
                    base_branch: reference.to_owned(),
                    since: None,
                },
                &NeverCancel,
                &GitLimits::default(),
            )
            .expect_err("invalid reference");
            assert!(matches!(error, GitError::InvalidReference));
        }
    }

    #[test]
    fn detached_linked_worktree_and_shallow_contexts_are_stable() {
        let temp = tempfile::tempdir().expect("temp");
        let main = temp.path().join("main repo");
        fs::create_dir(&main).expect("main repo");
        git(&main, &["init", "-q", "-b", "main"]);
        for revision in 0..3 {
            fs::write(
                main.join("lib.rs"),
                format!("fn value() {{ /* {revision} */ }}\n"),
            )
            .expect("source");
            git(&main, &["add", "lib.rs"]);
            git(
                &main,
                &["commit", "-q", "-m", &format!("revision {revision}")],
            );
        }

        let linked = temp.path().join("linked worktree");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/worktree",
                linked.to_str().expect("linked path"),
            ],
        );
        let main_context =
            resolve_context(&main, &NeverCancel, &GitLimits::default()).expect("main context");
        let linked_context =
            resolve_context(&linked, &NeverCancel, &GitLimits::default()).expect("linked context");
        assert!(linked_context.is_worktree);
        assert_eq!(linked_context.branch_slug, "feature-worktree");
        assert_eq!(linked_context.canonical_root, main_context.canonical_root);

        git(&linked, &["checkout", "--detach", "-q"]);
        let detached = resolve_context(&linked, &NeverCancel, &GitLimits::default())
            .expect("detached context");
        assert!(detached.is_detached);
        assert_eq!(detached.branch, "DETACHED");
        assert_eq!(detached.branch_slug, "detached");

        let shallow = temp.path().join("shallow clone");
        let status = Command::new("git")
            .args(["clone", "-q", "--no-local", "--depth=1"])
            .arg(&main)
            .arg(&shallow)
            .status()
            .expect("shallow clone");
        assert!(status.success());
        assert!(shallow.join(".git/shallow").exists());
        let shallow_context = resolve_context(&shallow, &NeverCancel, &GitLimits::default())
            .expect("shallow context");
        assert!(shallow_context.is_git);
        assert!(!shallow_context.head_sha.is_empty());
        let changes = detect_changes(
            &shallow,
            &DetectChangesOptions {
                base_branch: "no-such-branch".to_owned(),
                since: Some("HEAD".to_owned()),
            },
            &NeverCancel,
            &GitLimits::default(),
        )
        .expect("shallow HEAD diff");
        assert!(changes.failure.is_none());
    }
}
