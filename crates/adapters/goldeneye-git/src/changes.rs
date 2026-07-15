use std::{collections::BTreeSet, ffi::OsString, path::Path};

use super::{
    Cancellation, ChangedFile, ChangedHunk, DetectChangesOptions, DetectedChanges, GitError,
    GitFailure, GitLimits,
    history::is_trackable_file,
    process::{Capture, run_git, status_code},
};

/// Detects committed, local, staged, and untracked changes relative to a reference.
///
/// # Errors
///
/// Returns [`GitError::InvalidReference`] for an unsafe reference, or a Git
/// execution error when collection fails, is cancelled, times out, or exceeds limits.
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
    let mut files = BTreeSet::new();
    let base = base_changes(root, reference, cancellation, limits)?;
    add_nul_paths(&base.stdout, &mut files);
    let local = local_changes(root, cancellation, limits)?;
    add_nul_paths(&local.stdout, &mut files);
    let status = status_changes(root, cancellation, limits)?;
    add_status_paths(&status.stdout, &mut files);
    let failure = (!base.status.success() && files.is_empty()).then(|| GitFailure {
        status: status_code(base.status),
        reference: reference.to_owned(),
    });
    Ok(DetectedChanges {
        files: files.into_iter().collect(),
        failure,
    })
}

fn base_changes(
    root: &Path,
    reference: &str,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Capture, GitError> {
    let range = format!("{reference}...HEAD");
    let args = [
        OsString::from("diff"),
        OsString::from("--name-only"),
        OsString::from("-z"),
        OsString::from("--find-renames"),
        OsString::from(range),
        OsString::from("--"),
    ];
    run_git(root, &args, cancellation, limits)
}

fn local_changes(
    root: &Path,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Capture, GitError> {
    let args = [
        OsString::from("diff"),
        OsString::from("--name-only"),
        OsString::from("-z"),
        OsString::from("--find-renames"),
        OsString::from("--"),
    ];
    run_git(root, &args, cancellation, limits)
}

fn status_changes(
    root: &Path,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Capture, GitError> {
    let args = [
        OsString::from("--no-optional-locks"),
        OsString::from("status"),
        OsString::from("--porcelain=v1"),
        OsString::from("-z"),
        OsString::from("--untracked-files=normal"),
        OsString::from("--"),
    ];
    run_git(root, &args, cancellation, limits)
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
        if let Some(hunk) = parse_hunk(line, path) {
            hunks.push(hunk);
        }
    }
    hunks
}

fn parse_hunk(line: &str, path: &str) -> Option<ChangedHunk> {
    let plus = line.find('+')?;
    let range = line[(plus + 1)..]
        .split_whitespace()
        .next()
        .unwrap_or_default();
    let (start, count) = parse_range(range);
    if start == 0 {
        return None;
    }
    Some(ChangedHunk {
        path: path.to_owned(),
        start_line: start,
        end_line: start.saturating_add(count.saturating_sub(1)).max(start),
    })
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
