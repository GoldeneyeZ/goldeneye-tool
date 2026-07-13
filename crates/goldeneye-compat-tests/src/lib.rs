//! Frozen black-box compatibility support for Goldeneye.

use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const NORMALIZED_VERSION: &str = "<normalized>";

/// Returns the Cargo workspace root containing this compatibility crate.
///
/// # Panics
///
/// Panics if the crate is moved out of the expected `crates/<name>` layout.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("compatibility crate must live at crates/<name>")
        .to_path_buf()
}

/// Replays newline-delimited MCP requests through Goldeneye in memory.
///
/// # Errors
///
/// Returns an error when the fixture cannot be read, Goldeneye rejects the
/// session transport, or a nonempty response line is not valid JSON.
pub fn run_jsonl(requests: &Path) -> io::Result<Vec<Value>> {
    let input = fs::read(requests)?;
    let mut output = Vec::new();
    goldeneye::run_session(input.as_slice(), &mut output)
        .map_err(|error| io::Error::other(error.to_string()))?;
    parse_jsonl(&output)
}

/// Reads nonempty JSON lines from `path` in their original order.
///
/// # Errors
///
/// Returns an error when the file cannot be read, is not UTF-8, or contains an
/// invalid nonempty JSON line.
pub fn read_jsonl(path: &Path) -> io::Result<Vec<Value>> {
    parse_jsonl(&fs::read(path)?)
}

/// Normalizes only the build-dependent MCP server version.
///
/// IDs, protocol versions, results, errors, pagination fields, schemas, and
/// response order remain byte-semantically represented by their JSON values.
#[must_use]
pub fn normalize(mut values: Vec<Value>) -> Vec<Value> {
    for value in &mut values {
        if let Some(Value::String(version)) = value.pointer_mut("/result/serverInfo/version") {
            NORMALIZED_VERSION.clone_into(version);
        }
    }
    values
}

fn parse_jsonl(bytes: &[u8]) -> io::Result<Vec<Value>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid JSON on line {}: {error}", index + 1),
                )
            })
        })
        .collect()
}
