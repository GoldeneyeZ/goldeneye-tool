### Task 1: Create Discovery Crate and Public Domain Types

<TASK-ID>GD-1</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/Cargo.toml`
- Create: `crates/goldeneye-discovery/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `crates/goldeneye-discovery/src/lib.rs`

- [ ] **Step 1: Create crate manifest**

```toml
[package]
name = "goldeneye-discovery"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
ignore = "0.4.28"
thiserror.workspace = true

[dev-dependencies]
tempfile = "3.20"

[lints]
workspace = true
```

- [ ] **Step 2: Write failing public-type tests**

```rust
#[test]
fn defaults_match_upstream_discovery_limits() {
    let options = DiscoveryOptions::default();
    assert_eq!(options.mode, IndexMode::Full);
    assert_eq!(options.max_file_bytes, 512 * 1024 * 1024);
    assert!(!options.follow_symlinks);
    assert!(options.collect_ignored);
}

#[test]
fn max_file_bytes_accepts_positive_env_only() {
    assert_eq!(parse_max_file_bytes(Some("4096")), 4096);
    assert_eq!(parse_max_file_bytes(Some("0")), 512 * 1024 * 1024);
    assert_eq!(parse_max_file_bytes(Some("-1")), 512 * 1024 * 1024);
    assert_eq!(parse_max_file_bytes(Some("invalid")), 512 * 1024 * 1024);
}
```

- [ ] **Step 3: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery`

Expected: FAIL because discovery types/functions are undefined.

- [ ] **Step 4: Implement public types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
    Moderate,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageId(String);

impl LanguageId {
    pub fn new(value: impl Into<String>) -> Result<Self, DiscoveryError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DiscoveryError::InvalidLanguageId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    pub mode: IndexMode,
    pub max_file_bytes: u64,
    pub follow_symlinks: bool,
    pub collect_ignored: bool,
    pub global_ignore_path: Option<PathBuf>,
    pub extension_overrides: HashMap<OsString, LanguageId>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            mode: IndexMode::Full,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            follow_symlinks: false,
            collect_ignored: true,
            global_ignore_path: None,
            extension_overrides: HashMap::new(),
        }
    }
}

pub const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

pub fn parse_max_file_bytes(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_FILE_BYTES)
}
```

Define `DiscoveryError` with typed invalid-root, non-directory-root, invalid-language-data, ignore-rule, and I/O variants. Define report types without walker behavior:

```rust
pub struct DiscoveredFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub language: LanguageId,
    pub byte_len: u64,
}

pub enum IgnoreReason {
    IgnoreRule,
    DirectoryPolicy,
    SuffixPolicy,
    FilenamePolicy,
    PatternPolicy,
    Oversized,
    UnsupportedLanguage,
    Symlink,
    Io,
}

pub struct IgnoredPath {
    pub relative_path: PathBuf,
    pub reason: IgnoreReason,
    pub detail: Option<String>,
}

pub struct DiscoveryReport {
    pub root: PathBuf,
    pub files: Vec<DiscoveredFile>,
    pub excluded_directories: Vec<PathBuf>,
    pub ignored: Vec<IgnoredPath>,
    pub ignored_total: usize,
    pub warnings: Vec<String>,
}
```

- [ ] **Step 5: Verify types**

Run: `cargo fmt --check && cargo clippy -p goldeneye-discovery --all-targets -- -D warnings && cargo test -p goldeneye-discovery`

Expected: all commands exit 0.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/goldeneye-discovery
git commit -m "[GD-1] feat: define repository discovery domain"
```
