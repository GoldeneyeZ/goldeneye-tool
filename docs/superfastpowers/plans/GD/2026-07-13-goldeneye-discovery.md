# Goldeneye Repository Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:subagent-driven-development (recommended), superfastpowers:goal-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port upstream repository discovery and language classification into a deterministic, cross-platform Rust crate that preserves ignore precedence and fast/full selection behavior.

**Architecture:** `goldeneye-discovery` owns language registry, ignore evaluation, file walking, policy filters, and discovery reports. It uses `ignore::WalkBuilder` for Git-compatible walking, a separate high-precedence `.cbmignore` matcher so negations can override built-in skip policies, and checked-in generated language data derived from audited upstream tables.
**Plan Acronym:** GD


**Tech Stack:** Rust 1.97.0, edition 2024, `ignore 0.4.28`, `tempfile` for tests, standard `PathBuf`/`OsStr` APIs, Python 3 maintenance generator.

---

## File Structure

- `crates/goldeneye-discovery/Cargo.toml`: crate dependencies and test dependencies.
- `crates/goldeneye-discovery/src/lib.rs`: public discovery API.
- `crates/goldeneye-discovery/src/language.rs`: immutable registry and filename classification.
- `crates/goldeneye-discovery/src/ignore_rules.rs`: Git/global/nested/`.cbmignore` precedence.
- `crates/goldeneye-discovery/src/policy.rs`: full/moderate/fast directory, suffix, filename, and substring policies.
- `crates/goldeneye-discovery/src/walker.rs`: deterministic repository traversal and reporting.
- `crates/goldeneye-discovery/data/languages.tsv`: 160 upstream language rows with 239 extensions, 33 exact filenames, and compound mappings.
- `tools/export_upstream_languages.py`: reproducible extraction from audited upstream `cbm.h` and `language.c`.
- `crates/goldeneye-discovery/tests/language_parity.rs`: registry counts and representative mappings.
- `crates/goldeneye-discovery/tests/ignore_parity.rs`: root/nested/global/custom ignore precedence.
- `crates/goldeneye-discovery/tests/discovery_parity.rs`: modes, policies, symlinks, size caps, sorting, reports.
- `THIRD_PARTY.md`: `ignore` crate and transitive license ledger.
- `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/`: task packages and final review.

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

### Task 2: Port Complete Language Registry

<TASK-ID>GD-2</TASK-ID>

**Files:**
- Create: `tools/export_upstream_languages.py`
- Create: `crates/goldeneye-discovery/data/languages.tsv`
- Create: `crates/goldeneye-discovery/src/language.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Create: `crates/goldeneye-discovery/tests/language_parity.rs`

- [ ] **Step 1: Write failing registry parity tests**

```rust
#[test]
fn registry_matches_audited_upstream_counts() {
    let registry = LanguageRegistry::upstream();
    assert_eq!(registry.language_count(), 160);
    assert_eq!(registry.extension_count(), 239);
    assert_eq!(registry.filename_count(), 33);
    assert_eq!(registry.compound_extension_count(), 1);
}

#[test]
fn filename_extension_and_compound_precedence_match_upstream() {
    let registry = LanguageRegistry::upstream();
    assert_eq!(registry.classify(Path::new("main.rs")).unwrap().as_str(), "rust");
    assert_eq!(registry.classify(Path::new("CMakeLists.txt")).unwrap().as_str(), "cmake");
    assert_eq!(registry.classify(Path::new(".env")).unwrap().as_str(), "dotenv");
    assert_eq!(registry.classify(Path::new("view.blade.php")).unwrap().as_str(), "blade");
    assert_eq!(registry.classify(Path::new("unknown.binary")), None);
}

#[test]
fn explicit_extension_override_wins() {
    let mut overrides = HashMap::new();
    overrides.insert(OsString::from(".mjs"), LanguageId::new("typescript").unwrap());
    let registry = LanguageRegistry::with_overrides(overrides).unwrap();
    assert_eq!(registry.classify(Path::new("index.mjs")).unwrap().as_str(), "typescript");
}
```

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery --test language_parity`

Expected: FAIL because registry/data do not exist.

- [ ] **Step 3: Implement reproducible exporter**

`tools/export_upstream_languages.py` accepts `--upstream` and `--output`. It must:

1. Parse `internal/cbm/cbm.h` enum order from `CBM_LANG_GO` through item before `CBM_LANG_COUNT`.
2. Parse 160 display names from `LANG_NAMES` in `src/discover/language.c`.
3. Parse 239 `EXT_TABLE` entries, 33 `FILENAME_TABLE` entries, and compound `.blade.php` entry.
4. Emit UTF-8 TSV sorted by enum order with header:
   `id<TAB>display_name<TAB>extensions<TAB>filenames<TAB>compound_extensions`.
5. Normalize IDs from `CBM_LANG_FOO_BAR` to lowercase `foo_bar`.
6. Fail unless counts equal `160/239/33/1`.
7. Include comments recording upstream repository and commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.

Run:

```bash
python tools/export_upstream_languages.py --upstream .upstream/codebase-memory-mcp --output crates/goldeneye-discovery/data/languages.tsv
```

Expected: generated file contains 160 language rows and stable LF line endings.

- [ ] **Step 4: Implement immutable registry**

`LanguageRegistry` loads `include_str!("../data/languages.tsv")` once via `OnceLock` and builds:

- `HashMap<OsString, LanguageId>` for extensions;
- `HashMap<OsString, LanguageId>` for exact filenames;
- longest-first vector for compound extensions;
- `HashMap<LanguageId, LanguageSpec>` for display metadata.

Classification order: explicit override → exact filename → compound extension → last extension. Extension keys include leading dots and compare ASCII case-sensitively, matching upstream tables.

- [ ] **Step 5: Add exporter reproducibility test**

The test runs exporter against local read-only upstream checkout when present and compares bytes to checked-in TSV. When checkout is absent, it validates embedded provenance/counts and does not fail CI solely because `.upstream` is intentionally excluded.

- [ ] **Step 6: Verify registry**

Run: `cargo test -p goldeneye-discovery --test language_parity && cargo clippy -p goldeneye-discovery --all-targets -- -D warnings`

Expected: count, mapping, precedence, override, and reproducibility tests pass.

- [ ] **Step 7: Commit**

```bash
git add tools/export_upstream_languages.py crates/goldeneye-discovery
git commit -m "[GD-2] feat: port upstream language registry"
```

### Task 3: Implement Ignore Precedence and Policy Matchers

<TASK-ID>GD-3</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/src/ignore_rules.rs`
- Create: `crates/goldeneye-discovery/src/policy.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Create: `crates/goldeneye-discovery/tests/ignore_parity.rs`

- [ ] **Step 1: Write failing ignore precedence tests**

```rust
#[test]
fn nested_gitignore_stacks_with_root() {
    let repo = fixture([
        (".gitignore", "root.log\n"),
        ("src/.gitignore", "generated/\n"),
        ("root.log", "x"),
        ("src/generated/x.rs", "fn x() {}"),
        ("src/main.rs", "fn main() {}"),
    ]);
    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();
    assert!(rules.is_ignored(Path::new("root.log"), false));
    assert!(rules.is_ignored(Path::new("src/generated"), true));
    assert!(!rules.is_ignored(Path::new("src/main.rs"), false));
}

#[test]
fn cbmignore_negates_global_and_builtin_skips() {
    let repo = fixture([
        (".cbmignore", "!vendor/\n!vendor/keep.rs\n"),
        ("vendor/keep.rs", "fn keep() {}"),
    ]);
    let global = write_external_ignore("vendor/\n");
    let options = DiscoveryOptions {
        global_ignore_path: Some(global),
        ..DiscoveryOptions::default()
    };
    let rules = IgnoreRules::build(repo.path(), &options).unwrap();
    assert!(rules.is_explicitly_whitelisted(Path::new("vendor"), true));
    assert!(!rules.is_ignored(Path::new("vendor/keep.rs"), false));
}
```

Also cover comments, escaped `!`/`#`, rooted patterns, directory-only patterns, `**`, last-match-wins, non-Git repositories, and nested `.cbmignore`.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery --test ignore_parity`

Expected: FAIL because ignore rules are undefined.

- [ ] **Step 3: Implement ignore engine**

Use `ignore::WalkBuilder` configured with:

```rust
let mut builder = ignore::WalkBuilder::new(root);
builder
    .hidden(false)
    .follow_links(options.follow_symlinks)
    .git_ignore(true)
    .git_exclude(true)
    .git_global(options.global_ignore_path.is_none())
    .parents(true)
    .add_custom_ignore_filename(".cbmignore");
if let Some(path) = &options.global_ignore_path {
    builder.add_ignore(path);
}
```

Build a second `CbmIgnoreIndex` by pre-scanning only `.cbmignore` files without following symlinks and adding each file through `ignore::gitignore::GitignoreBuilder`. Use `matched_path_or_any_parents` to expose whether a path is explicitly whitelisted. This whitelist check runs before built-in directory/suffix/mode policies, preserving upstream behavior where `.cbmignore` negates global, always-skip, fast-skip, and earlier custom rules.

- [ ] **Step 4: Port policy tables exactly**

`policy.rs` defines audited arrays from `src/discover/discover.c:31-108`:

- 73 always-skip directory names;
- 40 moderate/fast skip directory names;
- 31 always-ignored suffixes;
- 47 moderate/fast ignored suffixes;
- 34 moderate/fast skip filenames;
- 15 moderate/fast substring patterns;
- 29 ignored JSON filenames.

Expose:

```rust
pub fn directory_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason>;
pub fn file_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason>;
```

Policy matching remains case-sensitive. `Full` applies always lists only. `Moderate` and `Fast` apply both always and fast lists, matching upstream `cbm_should_skip_dir`.

- [ ] **Step 5: Verify ignore/policy behavior**

Run: `cargo test -p goldeneye-discovery --test ignore_parity`

Expected: all precedence and policy tests pass, including `.cbmignore` negation.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-discovery
git commit -m "[GD-3] feat: preserve discovery ignore precedence"
```

### Task 4: Implement Deterministic Repository Walker

<TASK-ID>GD-4</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/src/walker.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Create: `crates/goldeneye-discovery/tests/discovery_parity.rs`

- [ ] **Step 1: Write failing walker tests**

```rust
#[test]
fn discovery_returns_supported_files_sorted_by_relative_path() {
    let repo = fixture([
        ("z.rs", "fn z() {}"),
        ("a.py", "def a(): pass"),
        ("notes.unknown", "ignored"),
        (".env", "A=1"),
    ]);
    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();
    let paths: Vec<_> = report.files.iter().map(|f| f.relative_path.as_path()).collect();
    assert_eq!(paths, [Path::new(".env"), Path::new("a.py"), Path::new("z.rs")]);
}

#[test]
fn discovery_skips_symlinks_and_oversized_files() {
    let repo = fixture([("small.rs", "fn x() {}"), ("large.rs", "0123456789")]);
    create_symlink(repo.path().join("small.rs"), repo.path().join("link.rs"));
    let options = DiscoveryOptions { max_file_bytes: 5, ..DiscoveryOptions::default() };
    let report = discover(repo.path(), &options).unwrap();
    assert!(report.files.is_empty());
    assert!(report.ignored.iter().any(|x| x.reason == IgnoreReason::Oversized));
    assert!(report.ignored.iter().any(|x| x.reason == IgnoreReason::Symlink));
}
```

Add tests for invalid root, file-as-root, Unicode/CJK paths, paths with spaces, empty files, unreadable entries where platform permits, `Full` vs `Moderate/Fast`, exact filenames, suffix filters, and `.cbmignore` recovery of built-in skipped dirs.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery --test discovery_parity`

Expected: FAIL because `discover` is undefined.

- [ ] **Step 3: Implement walker**

`discover` must:

1. Canonicalize root and require directory.
2. Build `IgnoreRules` and `LanguageRegistry` once.
3. Walk without following symlinks by default.
4. Record symlink entries instead of opening them.
5. Apply `.cbmignore` whitelist before policy filters.
6. Apply ignore rules, directory policy, file policy, metadata/size cap, then language classification.
7. Store canonical absolute path and root-relative `PathBuf` without lossy string conversion.
8. Continue after per-entry metadata/read errors; record warning/ignored entry.
9. Sort files, excluded directories, and ignored paths by platform path ordering normalized to forward-slash bytes for deterministic fixtures.
10. Set `ignored_total` before any report-detail cap.

Do not read file contents in discovery. Byte length comes from metadata; syntax phase owns reads.

- [ ] **Step 4: Add bounded ignored detail**

Expose `MAX_IGNORED_DETAILS: usize = 500`. Keep exact `ignored_total` while retaining at most 500 sorted `IgnoredPath` details. This prevents huge ignored trees from consuming MCP context later.

- [ ] **Step 5: Verify walker**

Run: `cargo fmt --check && cargo clippy -p goldeneye-discovery --all-targets -- -D warnings && cargo test -p goldeneye-discovery`

Expected: all language, ignore, policy, and walker tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-discovery
git commit -m "[GD-4] feat: discover repositories deterministically"
```

### Task 5: Freeze Upstream Discovery Parity and Legal Evidence

<TASK-ID>GD-5</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/tests/upstream_parity.rs`
- Create: `crates/goldeneye-discovery/tests/fixtures/discovery/manifest.tsv`
- Modify: `THIRD_PARTY.md`
- Modify: `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/plan-progression.md`

- [ ] **Step 1: Build frozen parity fixture**

Create one fixture repository in test setup covering:

- root and nested `.gitignore`;
- global ignore supplied explicitly;
- root and nested `.cbmignore` negation;
- always-skip and fast-skip directories;
- always and fast suffixes;
- fast filenames/patterns;
- supported, unsupported, exact-name, compound, hidden, Unicode, and spaced paths;
- symlink;
- oversized file.

`manifest.tsv` records each path, mode, expected disposition, language ID, and ignore reason. Each row cites matching upstream test or source policy line in a comment column.

- [ ] **Step 2: Write failing fixture replay**

```rust
#[test]
fn full_moderate_and_fast_reports_match_frozen_upstream_manifest() {
    let fixture = UpstreamFixture::materialize();
    for mode in [IndexMode::Full, IndexMode::Moderate, IndexMode::Fast] {
        let actual = discover(fixture.root(), &fixture.options(mode)).unwrap();
        assert_eq!(normalize_report(actual), fixture.expected(mode));
    }
}
```

- [ ] **Step 3: Run replay and verify RED**

Run: `cargo test -p goldeneye-discovery --test upstream_parity`

Expected: FAIL until all manifest rows and normalization are wired.

- [ ] **Step 4: Implement fixture helpers and repair only proven differences**

Normalization may convert path separators to `/` and omit platform-specific permission warnings. It must preserve file membership, language, exclusion reason, mode, ordering, size, and ignored totals.

- [ ] **Step 5: Update legal ledger**

Add `ignore 0.4.28` and transitive crates with licenses/source links. Record language TSV as MIT-derived data from audited upstream commit and preserve upstream notice.

- [ ] **Step 6: Run complete discovery gate**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; frozen discovery parity passes for all modes.

- [ ] **Step 7: Commit**

```bash
git add crates/goldeneye-discovery tools/export_upstream_languages.py THIRD_PARTY.md docs/superfastpowers/plans/GD
git commit -m "[GD-5] test: freeze repository discovery parity"
```

### Task 6: Repair Generated Language TSV Whitespace

<TASK-ID>GD-6</TASK-ID>

**Files:**
- Modify: `tools/export_upstream_languages.py`
- Modify: `crates/goldeneye-discovery/data/languages.tsv`
- Modify: `crates/goldeneye-discovery/src/language.rs`
- Modify: `crates/goldeneye-discovery/tests/language_parity.rs`

- [ ] **Step 1: Add failing generated-data hygiene test**

```rust
#[test]
fn generated_language_data_has_no_trailing_whitespace() {
    for (index, line) in include_str!("../data/languages.tsv").lines().enumerate() {
        assert_eq!(line.trim_end(), line, "trailing whitespace on TSV line {}", index + 1);
    }
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p goldeneye-discovery --test language_parity generated_language_data_has_no_trailing_whitespace`

Expected: FAIL on first language row whose empty final columns are encoded as trailing tabs.

- [ ] **Step 3: Fix root cause at exporter/parser boundary**

Exporter writes `-` for each empty list field instead of an empty final TSV cell. Registry parser treats an exact `-` field as an empty list and rejects mixed sentinel/data fields.

Regenerate:

```bash
python tools/export_upstream_languages.py --upstream .upstream/codebase-memory-mcp --output crates/goldeneye-discovery/data/languages.tsv
```

- [ ] **Step 4: Verify exact data parity and clean phase diff**

Run:

```bash
cargo test -p goldeneye-discovery --test language_parity
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check 13d741d..HEAD
```

Expected: registry remains `160/239/33/1`; exporter reproduction passes; every command exits 0.

- [ ] **Step 5: Commit**

```bash
git add tools/export_upstream_languages.py crates/goldeneye-discovery docs/superfastpowers/plans/GD
git commit -m "[GD-6] fix: remove generated TSV trailing whitespace"
```
