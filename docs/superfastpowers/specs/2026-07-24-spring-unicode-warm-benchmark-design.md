# Spring Unicode Warm Benchmark Design

**Date:** 2026-07-24
**Status:** User-approved design awaiting written-spec review
**Owner:** `goldeneye-tool/bench-owner`

## Goal

Measure the frozen Goldeneye plus ACK candidate on one routine Spring Framework
maintenance task from an identical immutable warm index for every scored
repetition. Compare correctness and agent behavior with the valid cached vanilla
baseline. Preserve enough provenance and raw evidence to reproduce or reject
the result.

## Approved Scope

### Spring task

Use Spring Framework commit
`daf955157871e4ac6f192e06b71d6cc595eb979b`.

Change `spring-core` method
`StringUtils.truncate(CharSequence, int)` so truncation never divides a UTF-16
surrogate pair at the threshold. Preserve:

- the existing positive-threshold precondition and messages;
- the existing `" (truncated)..."` suffix;
- existing behavior for BMP text and thresholds outside a surrogate-pair
  boundary;
- the existing return behavior when input length does not exceed the threshold.

Add focused `StringUtilsTests`. The task-facing verification command is:

```text
./gradlew :spring-core:test --tests org.springframework.util.StringUtilsTests
```

### Benchmark

- Engine: frozen Goldeneye plus ACK candidate.
- Cache condition: warm only.
- Repetitions: three serial scored repetitions under one configuration.
- Comparison: cached vanilla baseline using the same Spring commit, task, model,
  reasoning level, and grading contract.
- No Serena or Codebase Memory reruns.
- Candidate sources and binaries remain frozen during harness work and scoring.

## Task Implementation Design

Three implementation approaches were considered:

1. **Boundary-only backoff — selected.** Start with the existing UTF-16
   threshold. Back off one code unit only when the last included code unit is a
   high surrogate and the next code unit is a low surrogate. This fixes the
   invalid boundary while retaining current UTF-16 threshold semantics.
2. Count Unicode code points. This is broader and would change threshold
   semantics for every supplementary character before the boundary.
3. Convert through another text representation. This adds allocation and
   complexity without improving the contract.

The selected implementation is deliberately local. With `length > threshold`
and `threshold > 0`, inspecting `threshold - 1` and `threshold` is safe. A
surrogate pair that does not cross the threshold leaves the existing substring
boundary unchanged.

Focused tests cover:

- a supplementary character crossing the truncation boundary;
- a supplementary character wholly before the boundary;
- BMP-only behavior remaining unchanged;
- existing suffix and precondition behavior.

The held-out benchmark grader independently checks the boundary case and current
contract. It must not rely solely on tests authored by the benchmark agent.

## Immutable Ready-Snapshot Harness

### Why the current lifecycle changes

Current `executeRun()` creates a unique worktree and cache per run, then
`primeIndex()` builds a fresh warm index. That does not prove identical warm
starting state across repetitions.

Goldeneye and ACK persist absolute project/worktree paths. Copying one database
to different worktree paths risks stale path references. Scored repetitions
therefore reuse one stable absolute serial-lane worktree path and one stable live
cache path. Runs remain isolated because the lane is reset between repetitions,
and execution remains serial.

### Preparation phase

Preparation occurs before scored timing:

1. Validate Spring repository cleanliness and exact base commit.
2. Capture frozen candidate provenance:
   - Goldeneye repository HEAD, dirty diff fingerprint, source fingerprint,
     release binary SHA-256, and binary diff fingerprint;
   - ACK repository HEAD, tracked diff fingerprint, untracked-file inventory
     and fingerprints, dependency lock fingerprint, and built entrypoint
     fingerprint;
   - task, grader, harness, configuration, model, and prompt fingerprints.
3. Create the stable detached Spring worktree at the approved base commit.
4. Create an isolated live cache containing `ACK_HOME`, ACK registry,
   `GOLDENEYE_DB_PATH`, and decoy Codebase Memory cache.
5. Run ACK initialization once against that exact stable worktree path.
6. Stop the ACK/backend process and confirm no writer remains.
7. Copy the complete ready cache into an immutable snapshot directory. Never
   hardlink snapshot files.
8. Create a sorted SHA-256 manifest for every snapshot file and record snapshot
   byte/file counts.

Preparation, dependency warming, Gradle cache warming, index creation, snapshot
copying, and hashing are maintenance work and excluded from scored timing.

### Per-repetition reset

Before every scored repetition:

1. Remove the prior detached worktree through Git and recreate it at the same
   stable absolute path and base commit.
2. Verify clean status and exact HEAD.
3. Delete the live cache within its validated benchmark root.
4. Copy the immutable snapshot directory into the live cache directory.
5. Verify every live-cache file against the immutable SHA-256 manifest.
6. Verify ACK registry, Goldeneye database, and expected project binding exist.
7. Verify the immutable snapshot manifest itself has not changed.
8. Start scoring immediately before `codex exec`.

The snapshot directory is read-only by harness policy. The agent receives only
the restored live copy. Any reset, hash, binding, cleanliness, or provenance
failure rejects the lane before scoring.

### Scored timing and telemetry

- `wall_ms`: `codex exec` spawn through exit.
- `verified_e2e_ms`: `wall_ms` plus held-out grader duration.
- Snapshot preparation, restore, validation, engine setup, and indexing:
  maintenance metrics only.
- Record correctness, command failures, protocol violations, timeouts, input and
  output tokens, cached input, reasoning tokens, tool calls, ACK calls, backend
  MCP calls, failed calls, result payload bytes/cardinality, patch size, dirty
  paths, and grader duration.
- Compare first search selection, search ordering, failed discovery commands,
  and total discovery turns with the valid cached vanilla baseline; do not
  interpret query latency alone as agent effectiveness.

### Artifact preservation

Preserve per-run:

- exact prompt;
- Codex JSONL plus stdout/stderr;
- patch, status, and patch statistics;
- grader stdout/stderr and exit status;
- complete metrics;
- pre-run and post-run worktree status;
- snapshot restore/verification record;
- configuration and provenance manifest.

Preserve benchmark-level:

- immutable snapshot manifest;
- randomized run order and seed;
- base commit;
- candidate fingerprints before and after all runs;
- summary and causal analysis.

## Safety and Contamination Gates

- Worktree/cache deletion uses validated paths contained by the configured
  benchmark roots.
- Runs are serial; no shared writer operates against the snapshot or live cache.
- Existing unrelated Goldeneye and ACK worktree changes are preserved.
- Harness changes must not modify ACK or Goldeneye candidate source.
- Candidate fingerprints must match before preparation, before each run, and
  after the benchmark.
- Spring source worktree must begin each run clean at the pinned commit.
- Any mismatch aborts scoring and reports a concrete blocker.

## Validation

Harness tests must prove:

- snapshot creation uses copies rather than hardlinks;
- restore replaces contaminated live cache content;
- restored file hashes match the immutable manifest;
- manifest or live-copy tampering is detected;
- worktree path and commit mismatches are rejected;
- warm snapshot runs skip `primeIndex()`;
- reset/setup/restore/index time is excluded from `wall_ms` and
  `verified_e2e_ms`;
- existing cold/warm behavior remains unchanged when snapshot mode is disabled.

Before the scored run:

1. Run focused harness tests.
2. Run the harness test suite.
3. Run one non-scored snapshot smoke with the frozen candidate.
4. Verify candidate fingerprints remain unchanged.

## Success Criteria

- Every scored repetition begins from a byte-identical verified live copy of the
  immutable ready snapshot.
- All scored patches satisfy the held-out Spring grader and protocol rules.
- No candidate contamination or unapproved source change occurs.
- Raw artifacts and provenance support independent audit.
- Report distinguishes maintenance time, agent wall time, verified E2E time,
  correctness, and discovery behavior.

## Non-Goals

- Changing ACK or Goldeneye candidate code during this benchmark.
- Running cold, Serena, or Codebase Memory lanes.
- Replacing the cached vanilla baseline when its parity checks pass.
- Expanding Spring task scope beyond `StringUtils.truncate` and focused tests.
- Changing truncation thresholds from UTF-16 code units to Unicode code points.
