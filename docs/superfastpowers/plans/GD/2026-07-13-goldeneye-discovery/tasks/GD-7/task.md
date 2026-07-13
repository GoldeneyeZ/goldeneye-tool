### Task 7: Repair Discovery Safety and Ignore Parity

<TASK-ID>GD-7</TASK-ID>

**Files:**
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Modify: `crates/goldeneye-discovery/src/ignore_rules.rs`
- Modify: `crates/goldeneye-discovery/src/policy.rs`
- Modify: `crates/goldeneye-discovery/src/walker.rs`
- Modify: `crates/goldeneye-discovery/tests/ignore_parity.rs`
- Modify: `crates/goldeneye-discovery/tests/discovery_parity.rs`
- Modify: `crates/goldeneye-discovery/tests/upstream_parity.rs`
- Modify: `crates/goldeneye-discovery/tests/fixtures/discovery/manifest.tsv`
- Modify: `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/final-review.md`

- [ ] **Step 1: Add failing tier-precedence tests**

Add tests proving:

1. `.cbmignore` cannot re-include a path ignored by root `.gitignore`.
2. `.cbmignore` cannot re-include a path ignored by nested `.gitignore`.
3. `.cbmignore` cannot re-include a path ignored by `.git/info/exclude`.
4. `.cbmignore` may re-include a path ignored only by configured global ignore.
5. Nested `.cbmignore` uses last/more-local match within the custom tier.

- [ ] **Step 2: Add failing safety-policy tests**

```rust
#[test]
fn cbmignore_cannot_unskip_safety_core_directories() {
    for directory in [".git", "node_modules", ".worktrees", ".claude-worktrees"] {
        let report = discover(fixture_with_negated_directory(directory).path(), &DiscoveryOptions::default()).unwrap();
        assert!(!report.files.iter().any(|file| file.relative_path.starts_with(directory)));
    }
}
```

Add file tests proving `.cbmignore` cannot resurrect:

- always suffix `.png` in Full;
- fast suffix `.zip` in Fast;
- fast filename `Cargo.lock` in Fast;
- fast pattern `.generated.` in Fast.

- [ ] **Step 3: Add failing symlink/root-containment tests**

Remove supported behavior for following links. Delete `DiscoveryOptions.follow_symlinks` and assert all file symlinks, directory symlinks, and Windows reparse/junction entries are skipped. On POSIX, include an outside-root file and directory target; neither may appear in results. Windows junction creation is conditional only on privilege/API availability.

- [ ] **Step 4: Add failing pre-scan pruning test**

Create excluded `node_modules` and `.git` trees containing nested `.cbmignore` files and an unreadable/deep subtree. Instrument test outcome through public report: excluded trees must be pruned without ignore-file I/O warnings and without recursion proportional to their depth. Keep fixture depth bounded in test while asserting implementation contains no recursive whole-repository pre-scan helper.

- [ ] **Step 5: Run focused tests and verify RED**

Run:

```bash
cargo test -p goldeneye-discovery --test ignore_parity
cargo test -p goldeneye-discovery --test discovery_parity
cargo test -p goldeneye-discovery --test upstream_parity
```

Expected: failures reproduce all five final-review findings.

- [ ] **Step 6: Implement upstream tier semantics**

Replace whole-repository ignore pre-scan with lazy, directory-scoped cached loading during policy-aware traversal. Required order:

For directories:

1. non-negatable safety core (`.git`, `node_modules`, `.worktrees`, `.claude-worktrees`);
2. built-in mode directory policy, recoverable only by custom whitelist outside safety core;
3. root Git ignore and `.git/info/exclude`, terminal;
4. nested Git ignore, terminal;
5. global ignore candidate;
6. `.cbmignore` positive match, terminal;
7. `.cbmignore` negative match may clear only global candidate;
8. global result.

For files:

1. suffix, filename, and fast-pattern policies, terminal and never recoverable;
2. root Git ignore and `.git/info/exclude`, terminal;
3. nested Git ignore, terminal;
4. global ignore candidate;
5. `.cbmignore` positive match, terminal;
6. `.cbmignore` negative match may clear only global candidate;
7. size cap;
8. global result.

Use `ignore::gitignore::GitignoreBuilder` per tier/file, cached by directory. Do not recursively discover ignore files before traversal. Loading errors for a visited directory become bounded report warnings; excluded directories are never opened for nested ignore discovery.

- [ ] **Step 7: Remove link-following option**

Remove `follow_symlinks` from `DiscoveryOptions`, `configured_walk_builder`, walker branches, tests, and docs. Always use symlink metadata and record `IgnoreReason::Symlink` without resolving/reading target. This matches upstream `safe_stat` and eliminates outside-root traversal.

- [ ] **Step 8: Extend frozen manifest and verify GREEN**

Add cited rows for all precedence conflicts, four safety-core directories, four file-policy negations, and outside-root symlink cases. Normalization remains path-separator-only plus platform-unavailable link creation.

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check 13d741d..HEAD
```

Expected: every command exits 0; all new safety/parity tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/goldeneye-discovery docs/superfastpowers/plans/GD
git commit -m "[GD-7] fix: enforce discovery safety precedence"
```
