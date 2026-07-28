# ABFI Spec Review

**Result:** checked  
**Reviewed:** 2026-07-28  
**Implementation range:** `95db37f..3cc0c46`  
**Also inspected:** uncommitted ABFI progression and task-context metadata.

## Independent evidence

- `9858179` performs a 99% similarity rename of `tools/benchmark-agent-tasks.mjs` to `tools/agent-bench/bin/benchmark-agent-tasks.mjs`. Its only runner changes are the seven in-boundary relative-import rewrites, the repository-root calculation required by the deeper location, and the runner's provenance self-reference. The competitor runner is a 100% similarity rename to `tools/agent-bench/bin/benchmark-competitors.mjs`.
- The permanent isolation contract passes: `node --test tools/agent-bench/isolation.test.mjs` (exit `0`). It verifies both new entrypoints exist, both legacy paths do not, relative imports stay within the bench root, production Rust sources have no bench references, and the package contract is exact.
- The complete package suite passes: `npm --prefix tools/agent-bench test` (exit `0`). The package syntax command passes: `npm --prefix tools/agent-bench run check` (exit `0`). The affected core suite independently passes: `node --test tools/agent-bench/core.test.mjs` (exit `0`).
- Tracked-reference checks: `git grep` found `0` matches each for `tools/benchmark-agent-tasks.mjs` and `tools/benchmark-competitors.mjs`; neither legacy file exists. The mechanical migrations cover active docs and the historical SSRB/SUWB plan packages.
- Physical isolation check found `0` tracked paths under `tools/` matching `bench` outside `tools/agent-bench/`; 52 tracked files now live in the bench directory.
- Cargo separation check with `cargo metadata --no-deps --format-version 1` found `0` packages or targets whose manifest/source path refers to `tools/agent-bench`, `benchmark-agent`, or `benchmark-competitors`. The implementation range contains no `Cargo.toml` or `crates/**` changes.
- `git diff --check 95db37f 3cc0c46` and the current-worktree `git diff --check` both exit `0`.
- The protected untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` remains untracked and was not added or modified by this review.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| One-folder benchmark runtime | PASS | Both entrypoints are under `tools/agent-bench/bin/`; no tracked bench runtime path remains directly under `tools/`. |
| No compatibility wrappers | PASS | Both legacy paths are absent from disk and from tracked files. |
| Every tracked caller migrated | PASS | Both exact legacy-path `git grep` searches return zero matches. |
| Existing behavior preserved | PASS | The move is content-preserving apart from required paths/root calculation; affected core and package tests pass. |
| Node package and operator README | PASS | `package.json` is private ESM, requires Node `>=20`, and provides test/check/both runner scripts; README documents commands, ownership, inputs/outputs, and Cargo separation. |
| Tests and syntax checks | PASS | Isolation, core, and full package tests pass; both entrypoints pass `node --check` through the package `check` script. |
| Cargo production separation | PASS | Contract and fresh Cargo metadata check both report no benchmark-tool production coupling. |
| Out-of-scope and protected file honored | PASS | No Rust/Cargo or scoring-behavior changes; the protected untracked call-tree remains untouched. |

No implementer handoff is required.
