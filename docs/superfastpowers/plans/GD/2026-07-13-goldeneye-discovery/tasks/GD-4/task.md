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
