use std::{
    ffi::OsString,
    io::{self, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::Instant,
};

use super::{Cancellation, GitError, GitLimits};

#[derive(Debug)]
pub(super) struct Capture {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
}

enum WaitOutcome {
    Complete(ExitStatus),
    Cancelled,
    TimedOut,
}

pub(super) fn capture_one(
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

pub(super) fn run_git(
    root: &Path,
    args: &[OsString],
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<Capture, GitError> {
    if cancellation.is_cancelled() {
        return Err(GitError::Cancelled);
    }
    let mut child = command(root, args).spawn().map_err(GitError::Spawn)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let limit = limits.max_output_bytes;
    let stdout_thread = thread::spawn(move || read_bounded(stdout, limit));
    let stderr_thread = thread::spawn(move || read_bounded(stderr, limit));
    let outcome = wait_for_child(&mut child, cancellation, limits)?;
    let (stdout, stdout_truncated) = join_reader(stdout_thread)?;
    let (stderr, stderr_truncated) = join_reader(stderr_thread)?;
    match outcome {
        WaitOutcome::Cancelled => Err(GitError::Cancelled),
        WaitOutcome::TimedOut => Err(GitError::TimedOut(limits.timeout)),
        WaitOutcome::Complete(status) => {
            if stdout_truncated || stderr_truncated {
                return Err(GitError::OutputLimit { limit });
            }
            let _ = stderr;
            Ok(Capture { status, stdout })
        }
    }
}

fn command(root: &Path, args: &[OsString]) -> Command {
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
    command
}

fn wait_for_child(
    child: &mut Child,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<WaitOutcome, GitError> {
    let started = Instant::now();
    loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::Cancelled);
        }
        if started.elapsed() >= limits.timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::TimedOut);
        }
        match child.try_wait().map_err(GitError::Output)? {
            Some(status) => return Ok(WaitOutcome::Complete(status)),
            None => thread::sleep(limits.poll_interval),
        }
    }
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

pub(super) fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}
