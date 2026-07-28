# ABFI Code Quality Review

**Result:** failed  
**Scope:** whole goal  
**Reviewed range:** `e4b8bb5..3cc0c46`, including ABFI-2 repair `4b26507` and current uncommitted ABFI review metadata.

## Findings

### Important — competitor runner no longer resolves the repository root

`tools/agent-bench/bin/benchmark-competitors.mjs:17` retained `resolve(dirname(fileURLToPath(import.meta.url)), "..")` after moving from `tools/` into `tools/agent-bench/bin/`. The old expression resolved to the repository root; the current expression resolves to `tools/agent-bench` instead.

Consequently, the unchanged defaults at `tools/agent-bench/bin/benchmark-competitors.mjs:72` and `:77` now point to `tools/agent-bench/target/...`, and the Cargo build at `:483` starts from the package directory rather than the intended repository root. This violates the relocation requirement to preserve runner behavior and breaks the default competitor-runner workflow unless callers override its paths.

The suite did not detect this regression: `isolation.test.mjs` checks physical placement/import boundaries and `core.test.mjs` exercises the agent runner, but neither asserts the competitor runner's repository-root-derived defaults or build CWD.

### Minor — README calls the new entrypoints legacy and nonexistent

`tools/agent-bench/README.md:42` says that `tools/agent-bench/bin/benchmark-agent-tasks.mjs` and `tools/agent-bench/bin/benchmark-competitors.mjs` no longer exist. Those are the new paths documented immediately above at lines 24–25. The obsolete paths are `tools/benchmark-agent-tasks.mjs` and `tools/benchmark-competitors.mjs`.

## Evidence

- Package contract is sound: the manifest is private ESM, requires Node `>=20`, and exposes local test/check/runner commands.
- Relative imports in both moved entrypoints stay within `tools/agent-bench/`; no production Cargo import boundary issue was found.
- `npm --prefix tools/agent-bench test` passed: 51 tests, 0 failures.
- `npm --prefix tools/agent-bench run check`, package-CWD `node --test`, and `git diff --check e4b8bb5..3cc0c46` all exited 0.
- The 32 changed documentation files are mechanical entrypoint-path migrations, except the ABFI task text that expressly requires preserving the location-derived workspace calculation. No unrelated runtime changes were found.
- Task contexts record the ABFI-2 package-CWD repair accurately for `core.test.mjs`, but their verification does not cover the competitor-runner default-path regression.

## Progression

Code quality is failed. Integration review remains unchecked; all ABFI tasks remain implemented under the bypass policy. The checked spec review remains valid because the defect is an implementation-quality failure, not a specification change.

## Re-review — 2026-07-28

**Result:** checked  
**Re-reviewed range:** `95db37f..6d51cf0`, including repairs `484853a` and `6d51cf0`, plus current uncommitted ABFI metadata.

The prior findings are resolved:

- `tools/agent-bench/bin/benchmark-competitors.mjs:17` now resolves `../../..` from `bin/`, which evaluates to the repository root. Its default Goldeneye path, default output path, and Cargo build CWD therefore retain their pre-relocation root semantics.
- `tools/agent-bench/competitors.test.mjs` is a focused regression test. It fails with the prior one-parent expression and verifies the repaired workspace, defaults, and Cargo build CWD without starting benchmark services.
- `tools/agent-bench/README.md:42` now accurately describes the obsolete paths as the previous top-level runners under `tools/`.

Fresh verification passed:

- `npm --prefix tools/agent-bench test`: 52 tests, 0 failures.
- `npm --prefix tools/agent-bench run check`: exit 0.
- `node --check` for both bin runners: exit 0.
- Exact tracked old-path search: 0 matches.
- Tracked benchmark-runtime paths outside `tools/agent-bench/`: 0.
- `cargo metadata --no-deps --format-version 1`: 0 benchmark-tool package, target, or dependency references.
- `git diff --check`: exit 0 (only existing CRLF conversion warnings for uncommitted ABFI metadata).

The task contexts accurately retain all tasks as implemented and record the repair provenance. The pre-existing untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` was preserved.

## Progression

Code quality is checked. Under the bypass policy, all tasks remain implemented and the next required gate is plan-scoped integration review.
