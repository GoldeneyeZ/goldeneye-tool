# Goldeneye Discovery Final Integration Review

**Reviewed range:** `13d741d..c7a8f41`  
**Head verified:** `c7a8f41b6641263d0b64a8ddc5de34ca0e15f40c`  
**Verdict:** **NOT READY** — 2 Critical, 3 Important, 0 Minor.

## Findings

### Critical

1. `crates/goldeneye-discovery/src/ignore_rules.rs:72`: `.cbmignore` whitelist results return `false` before any standard matcher is consulted, so a repository-controlled `.cbmignore` can re-include paths excluded by root/nested `.gitignore` (and `.git/info/exclude`). Upstream makes project/local Git ignore matches terminal and permits `.cbmignore` negation to cancel only the global ignore (`src/discover/discover.c:544-562`, `588-610`). This can index files deliberately excluded as secrets or generated artifacts. Preserve matcher tiers and upstream precedence: project/local Git ignores must win; `.cbmignore` may negate only the global tier. Add root, nested, and `.git/info/exclude` conflict tests.

2. `crates/goldeneye-discovery/src/walker.rs:140`: an explicit `.cbmignore` whitelist bypasses every built-in directory policy, including `.git`, `node_modules`, `.worktrees`, and `.claude-worktrees`. Upstream declares these four a non-negatable safety core specifically to prevent OOM, VCS-internal indexing, and duplicate worktree indexing (`src/discover/discover.c:514-543`). Check the safety-core set before whitelist recovery and add a regression case for each directory.

### Important

1. `crates/goldeneye-discovery/src/walker.rs:170`: `file_policy` runs only when the path is not whitelisted. Upstream applies always/fast suffix, filename, and pattern filters before all ignore matching (`src/discover/discover.c:571-610`), so `.cbmignore` cannot resurrect archives, binaries, generated bundles, or other policy-filtered files. Apply `file_policy` unconditionally before ignore evaluation; test negations for an always suffix, fast suffix, fast filename, and fast pattern.

2. `crates/goldeneye-discovery/src/walker.rs:118`: with `follow_symlinks = true`, file and directory links are followed without proving the canonical target remains under `DiscoveryReport.root`; `process_file` then stores the outside target at line 203. A repository link can therefore make discovery read/index arbitrary reachable filesystem content. Upstream always rejects symlinks/reparse points specifically to prevent project-root escape (`src/discover/discover.c:653-675`). Either remove the incompatible option or require canonical `target.starts_with(root)` for both file and directory links, with POSIX symlink and Windows junction tests.

3. `crates/goldeneye-discovery/src/ignore_rules.rs:225`: ignore-file discovery recursively scans every directory before the real walker applies Git, safety-core, or mode policies. Large `node_modules`, `.git`, build, and worktree trees are therefore traversed despite being excluded; recursion is unbounded, unlike upstream's bounded walk stack. Load nested ignore files lazily during the policy-aware walk, or use an iterative bounded scan that prunes excluded/safety-core directories. Add a fixture proving an excluded unreadable/deep tree is not pre-scanned.

## Evidence and coverage audit

- Git range and clean pre-review tree were verified; 13 phase commits changed 41 files.
- All seven Rust policy tables were independently parsed and compared with pinned upstream `discover.c`: exact counts and order match (`73/40/31/47/34/15/29`).
- The generated language ledger and registry freeze the requested `160` languages, `239` extensions, `33` exact filenames, and one compound extension; provenance points to pinned upstream commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.
- Deterministic sorting and ignored-detail truncation are implemented, but those properties do not close the precedence/root-containment defects above.
- Existing parity fixtures do not exercise the upstream non-negatable safety core, `.cbmignore` versus root/nested Git ignore conflicts, policy-file negation, or followed links escaping the repository. Their passing state is therefore truthful for covered cases but insufficient for release parity.
- `THIRD_PARTY.md` records the new direct `ignore` dependency and its transitive license closure; no legal blocker found in this range.

## Readiness

Do not advance GD. Fix both Critical and all Important findings, extend the frozen manifest/regression suite, then rerun formatting, clippy with warnings denied, workspace tests, release build, and the phase diff/legal gate.
