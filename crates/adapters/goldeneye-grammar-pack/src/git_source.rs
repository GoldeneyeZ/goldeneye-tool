//! Exact-Git object verification for grammar-pack assets.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use super::{
    BUFFER_SIZE, PackError, ensure_safe_absolute_components, ensure_safe_existing_directory,
    invalid, validate_relative_path,
};

#[derive(Clone)]
struct GitEntry {
    object_id: String,
    size: u64,
}

pub(super) struct GitSourceSession {
    repository: PathBuf,
    prefix: String,
    entries: BTreeMap<String, GitEntry>,
    batch: Option<GitBatch>,
}

impl GitSourceSession {
    pub(super) fn new(repository: &Path, prefix: &str, commit: &str) -> Result<Self, PackError> {
        validate_git_object_id("upstream commit", commit)?;
        validate_relative_path(prefix)?;
        let repository = canonical_git_repository(repository)?;
        let commit_spec = format!("{commit}^{{commit}}");
        let resolved = run_git_text(
            &repository,
            &["rev-parse", "--verify", "--end-of-options", &commit_spec],
        )?;
        if resolved != commit {
            return invalid(format!(
                "expected exact Git commit {commit}, resolved {resolved}"
            ));
        }

        let output = run_git(
            &repository,
            &["ls-tree", "-r", "-z", "--long", commit, "--", prefix],
        )?;
        let prefix_with_slash = format!("{prefix}/");
        let mut entries = BTreeMap::new();
        for record in output.stdout.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            let tab = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| PackError::Invalid("malformed NUL Git tree record".into()))?;
            let fields = record[..tab]
                .split(u8::is_ascii_whitespace)
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>();
            if fields.len() != 4 {
                return invalid("malformed Git tree metadata");
            }
            let mode = std::str::from_utf8(fields[0])
                .map_err(|_| PackError::Invalid("non-ASCII Git tree mode".into()))?;
            let object_type = std::str::from_utf8(fields[1])
                .map_err(|_| PackError::Invalid("non-ASCII Git object type".into()))?;
            let object_id = std::str::from_utf8(fields[2])
                .map_err(|_| PackError::Invalid("non-ASCII Git object ID".into()))?;
            let size = std::str::from_utf8(fields[3])
                .map_err(|_| PackError::Invalid("non-ASCII Git blob size".into()))?
                .parse::<u64>()
                .map_err(|_| PackError::Invalid("invalid Git blob size".into()))?;
            let path = std::str::from_utf8(&record[tab + 1..])
                .map_err(|_| PackError::Invalid("non-UTF-8 path in pinned Git tree".into()))?;
            let relative = path.strip_prefix(&prefix_with_slash).ok_or_else(|| {
                PackError::Invalid(format!("Git tree path escaped prefix {prefix:?}: {path:?}"))
            })?;
            validate_relative_path(relative)?;
            if object_type != "blob" || !matches!(mode, "100644" | "100755") {
                return invalid(format!(
                    "path is not a regular Git blob: {path} (mode {mode}, type {object_type})"
                ));
            }
            validate_git_object_id("blob object ID", object_id)?;
            if entries
                .insert(
                    relative.to_owned(),
                    GitEntry {
                        object_id: object_id.to_owned(),
                        size,
                    },
                )
                .is_some()
            {
                return invalid(format!("duplicate path in pinned Git tree: {path}"));
            }
        }

        Ok(Self {
            repository,
            prefix: prefix.to_owned(),
            entries,
            batch: None,
        })
    }

    pub(super) fn with_asset<T>(
        &mut self,
        grammar_name: &str,
        asset: &str,
        operation: impl FnOnce(u64, PathBuf, &mut dyn Read) -> Result<T, PackError>,
    ) -> Result<T, PackError> {
        let relative = format!("{grammar_name}/{asset}");
        let entry = self.entries.get(&relative).cloned().ok_or_else(|| {
            PackError::Invalid(format!(
                "missing pinned Git path: {}/{relative}",
                self.prefix
            ))
        })?;
        if self.batch.is_none() {
            self.batch = Some(GitBatch::spawn(&self.repository)?);
        }
        let display_path = self.repository.join(&self.prefix).join(&relative);
        self.batch
            .as_mut()
            .expect("batch was initialized")
            .with_blob(&entry, display_path, operation)
    }

    pub(super) fn finish(&mut self) -> Result<(), PackError> {
        if let Some(batch) = self.batch.as_mut() {
            batch.finish()?;
        }
        Ok(())
    }
}

struct GitBatch {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    reaped: bool,
}

impl GitBatch {
    fn spawn(repository: &Path) -> Result<Self, PackError> {
        let mut child = git_command(repository)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| PackError::Io {
                path: repository.to_path_buf(),
                source,
            })?;
        let Some(stdin) = child.stdin.take() else {
            kill_and_reap(&mut child);
            return invalid("git cat-file stdin was not piped");
        };
        let Some(stdout) = child.stdout.take() else {
            drop(stdin);
            kill_and_reap(&mut child);
            return invalid("git cat-file stdout was not piped");
        };
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::with_capacity(BUFFER_SIZE, stdout),
            reaped: false,
        })
    }

    fn with_blob<T>(
        &mut self,
        entry: &GitEntry,
        display_path: PathBuf,
        operation: impl FnOnce(u64, PathBuf, &mut dyn Read) -> Result<T, PackError>,
    ) -> Result<T, PackError> {
        let request = format!("{}\n", entry.object_id);
        let request_result = self
            .stdin
            .as_mut()
            .ok_or_else(|| PackError::Invalid("git cat-file batch is closed".into()))
            .and_then(|stdin| {
                stdin
                    .write_all(request.as_bytes())
                    .and_then(|()| stdin.flush())
                    .map_err(|source| PackError::Io {
                        path: display_path.clone(),
                        source,
                    })
            });
        if let Err(error) = request_result {
            self.abort();
            return Err(error);
        }

        let header = match read_limited_line(&mut self.stdout, 256) {
            Ok(header) => header,
            Err(source) => {
                self.abort();
                return Err(PackError::Io {
                    path: display_path,
                    source,
                });
            }
        };
        let expected_header = format!("{} blob {}\n", entry.object_id, entry.size);
        if header != expected_header.as_bytes() {
            self.abort();
            return invalid(format!(
                "unexpected git cat-file header for {}: {:?}",
                display_path.display(),
                String::from_utf8_lossy(&header)
            ));
        }

        let (result, remaining) = {
            let mut reader = self.stdout.by_ref().take(entry.size);
            let result = operation(entry.size, display_path.clone(), &mut reader);
            (result, reader.limit())
        };
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        if remaining != 0 {
            self.abort();
            return invalid(format!(
                "truncated git blob while reading {}: {remaining} bytes missing",
                display_path.display()
            ));
        }
        let mut delimiter = [0_u8; 1];
        if let Err(source) = self.stdout.read_exact(&mut delimiter) {
            self.abort();
            return Err(PackError::Io {
                path: display_path,
                source,
            });
        }
        if delimiter != *b"\n" {
            self.abort();
            return invalid(format!(
                "missing git cat-file delimiter after {}",
                display_path.display()
            ));
        }
        Ok(value)
    }

    fn finish(&mut self) -> Result<(), PackError> {
        if self.reaped {
            return Ok(());
        }
        drop(self.stdin.take());
        let status = self.child.wait().map_err(|source| PackError::Io {
            path: PathBuf::from("git cat-file --batch"),
            source,
        })?;
        self.reaped = true;
        if !status.success() {
            return invalid(format!("git cat-file --batch exited with {status}"));
        }
        Ok(())
    }

    fn abort(&mut self) {
        if self.reaped {
            return;
        }
        drop(self.stdin.take());
        kill_and_reap(&mut self.child);
        self.reaped = true;
    }
}

impl Drop for GitBatch {
    fn drop(&mut self) {
        self.abort();
    }
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_limited_line(reader: &mut impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut line = Vec::with_capacity(limit.min(96));
    for _ in 0..limit {
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte)?;
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(line);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "git cat-file header exceeded limit",
    ))
}

fn canonical_git_repository(path: &Path) -> Result<PathBuf, PackError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PackError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    ensure_safe_absolute_components(&absolute)?;
    let canonical = fs::canonicalize(&absolute).map_err(|source| PackError::Io {
        path: absolute,
        source,
    })?;
    ensure_safe_existing_directory(&canonical)?;
    let reported = run_git_text(&canonical, &["rev-parse", "--show-toplevel"])?;
    let reported = PathBuf::from(reported);
    ensure_safe_absolute_components(&reported)?;
    let reported = fs::canonicalize(&reported).map_err(|source| PackError::Io {
        path: reported,
        source,
    })?;
    if reported != canonical {
        return invalid(format!(
            "Git repository path must be its canonical worktree root: expected {}, got {}",
            reported.display(),
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1");
    command
}

fn run_git(repository: &Path, arguments: &[&str]) -> Result<std::process::Output, PackError> {
    let output = git_command(repository)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| PackError::Io {
            path: repository.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return invalid(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output)
}

fn run_git_text(repository: &Path, arguments: &[&str]) -> Result<String, PackError> {
    let output = run_git(repository, arguments)?;
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| PackError::Invalid("git returned non-UTF-8 text output".into()))?;
    Ok(stdout.trim().to_owned())
}

fn validate_git_object_id(kind: &str, object_id: &str) -> Result<(), PackError> {
    if !matches!(object_id.len(), 40 | 64)
        || !object_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return invalid(format!(
            "{kind} must be 40 or 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}
