# Context for SUWB-1

**Plan:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark.md`
**Task:** `SUWB-1`
**Commit SHA:** Pending until task completion. If review fixes add commits, update to the latest task commit and note the reviewed range below.

## Starting Context

- `tools/agent-bench/core.mjs`: existing benchmark path and lifecycle helpers.
- `tools/agent-bench/core.test.mjs`: existing Node test conventions.
- `docs/superfastpowers/specs/2026-07-24-spring-unicode-warm-benchmark-design.md`: approved fail-closed snapshot design.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Final task commit SHA: `7b43b5bc37466741c6461d0283ea24a0456a4c57`
- Reviewed commit range: `bddb34e43f316d333620b0a2289902cb178fd667..7b43b5bc37466741c6461d0283ea24a0456a4c57`
- Files created: `tools/agent-bench/snapshot.mjs`; `tools/agent-bench/snapshot.test.mjs`
- Files modified: none
- Additional relevant files: `tools/agent-bench/core.test.mjs` verified unchanged
- Verification commands/results: initial RED `node --test tools/agent-bench/snapshot.test.mjs` -> 6 failures, first expected missing API; initial GREEN -> 6 pass; repair RED after separate-root test -> 5 failures caused by `snapshot` containment against cache root; repair GREEN -> 7 pass; regression `node --test tools/agent-bench/core.test.mjs tools/agent-bench/snapshot.test.mjs` -> 15 pass; staged `git diff --cached --check` -> clean
- Notes: copy-only lifecycle rejects symlinks/junctions and writer artifacts, records sorted SHA-256 manifest, detects tamper/contamination, and restores a verified independent live copy. `allowedSnapshotRoot` now separately constrains create, verify, and restore; an out-of-root snapshot is rejected before live-cache deletion. Windows file-symlink EPERM path uses a real junction fallback; rejection proof passes.
