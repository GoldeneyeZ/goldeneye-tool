# Agent Bench One-Shot Mode Design

Date: 2026-07-30
Status: Approved

## Purpose

Add a fast, explicitly non-canonical benchmark mode that performs exactly one
model invocation while preserving repository isolation, GCAL warm-state setup,
telemetry collection, and one held-out grader execution.

The mode removes qualification-smoke overhead, agent-initiated build/test loops,
and report-ID collisions. It does not limit GCAL discovery calls.

## Goals

- Execute exactly one task, one engine, one cache mode, and one repetition.
- Invoke Codex exactly once.
- Automatically refresh a missing or stale GCAL snapshot without invoking Codex.
- Skip qualification smoke.
- Ask the agent not to run build, compile, test, lint, or check commands.
- Retain one held-out grader as the correctness authority.
- Generate a unique attempt ID, artifact directory, and standalone report.
- Never retry a failed model, grader, snapshot, or cleanup operation.
- Preserve existing scored, smoke, calibration, and audit workflows.

## Non-Goals

- No GCAL-call budget or GCAL-call limit.
- No replacement for canonical scored reports.
- No inclusion in the four-run benchmark audit.
- No hard interception of commands inside the Codex process.
- No grader skipping in this change.
- No automatic rerun after any failure.

## CLI Contract

New options:

```text
--one-shot                    run one non-canonical model invocation
--skip-agent-verification     instruct agent to skip build/test verification
--attempt-id <id>             optional unique artifact identifier
```

`--one-shot` implies `--skip-agent-verification`.

When `--attempt-id` is omitted, the runner generates a filesystem-safe identifier
from UTC time plus random entropy. The resolved attempt ID is stored in the
report.

`--one-shot` rejects the command before repository or model work unless the
resolved matrix contains exactly:

- one task;
- one selected engine;
- one cache mode;
- one repetition.

`--one-shot` is incompatible with:

- `--smoke`;
- `--calibration`;
- `--audit-report`;
- `--prepare-snapshot`;
- `--verify-only`.

`--dry-run --one-shot` validates and prints the single resolved run without
snapshot refresh or model execution.

## Execution Flow

```text
parse and validate one-shot CLI
  -> resolve exactly one matrix entry
  -> create unique one-shot output root
  -> resolve pinned source commit
  -> verify required snapshot
  -> refresh snapshot when missing or stale
  -> restore isolated worktree and cache
  -> invoke Codex exactly once
  -> collect patch, status, telemetry, and compliance data
  -> run held-out grader exactly once
  -> clean worktree, cache, and GCAL daemon
  -> write standalone one-shot report
```

No branch in this flow may invoke Codex more than once.

## Snapshot Behavior

Vanilla one-shot runs do not require GCAL preparation.

Warm GCAL one-shot runs verify the configured ready snapshot. When it is missing,
ineligible, or stale, the runner executes snapshot preparation without smoke.
Snapshot preparation may initialize GCAL and Goldeneye, copy files, checkpoint
SQLite, and write manifests, but it must not spawn Codex.

One-shot mode accepts a successfully prepared snapshot even though canonical
`eligible_for_scoring` remains false without smoke. The report records:

```json
{
  "qualification": "skipped",
  "snapshot_refreshed": true
}
```

Canonical scored mode retains the existing eligible-preparation requirement.

Snapshot refresh failure stops the command before model invocation.

## Agent Verification Policy

When `--skip-agent-verification` is active, the final prompt section is:

```text
Fast one-shot verification policy:
- Do not run build, compile, test, lint, check, grader, or validation commands.
- Finish after implementation and source review.
- The harness will run one held-out grader after your response.
```

This instruction appears after the task prompt so task-local verification text
cannot override it accidentally.

The current Codex integration cannot intercept individual shell commands before
execution. The runner therefore analyzes completed command telemetry and records:

```json
{
  "agent_verification_calls": [],
  "one_shot_compliant": true
}
```

Recognized verification commands include Gradle, Maven, Cargo, npm/pnpm/yarn
test/build/check/lint commands, and common direct test runners. Detection must
avoid classifying ordinary source inspection or Git commands as verification.

An agent verification attempt:

- is recorded with command and exit code;
- sets `one_shot_compliant` to false;
- does not trigger a retry;
- does not suppress the held-out grader;
- does not alone change grader-based success.

## GCAL Policy

GCAL discovery remains unrestricted. Existing protocol rules still require GCAL as
the code-discovery surface in the GCAL lane. No new counter, maximum, timeout, or
prompt instruction limits GCAL calls.

Existing GCAL telemetry remains present:

- `gcal_calls`;
- `gcal_failures`;
- action counts;
- protocol violations.

## Reporting and Artifact Isolation

Default one-shot output:

```text
target/agent-bench/<task-id>/one-shot/<attempt-id>/report.json
```

Run artifacts live below the same attempt root. One-shot mode never merges into
the canonical config report. This prevents `Duplicate scored run ID` failures
and preserves previous artifacts.

The one-shot report includes existing run metrics plus:

```json
{
  "mode": "one-shot",
  "attempt_id": "<resolved-id>",
  "qualified": false,
  "qualification": "skipped",
  "snapshot_refreshed": false,
  "model_invocations": 1,
  "grader_invocations": 1,
  "agent_verification_policy": "skip",
  "agent_verification_calls": [],
  "one_shot_compliant": true
}
```

One-shot report generation writes a new standalone report atomically. It does
not call canonical report merge logic.

## Success and Failure Semantics

Success requires:

- Codex exit code zero;
- no agent timeout;
- held-out grader exit code zero;
- existing dirty-path and protocol requirements.

`one_shot_compliant` is reported independently from success because command
interception is unavailable.

Failures:

- invalid matrix: fail before snapshot/model work;
- stale snapshot refresh failure: fail before model work;
- Codex failure or timeout: record failure, do not retry;
- grader failure: record failure, do not retry;
- report write failure: preserve run artifacts, return nonzero;
- cleanup failure: report exact target and return nonzero.

The report records actual invocation counts even on failure.

## Compatibility

Without `--one-shot`, behavior remains unchanged:

- canonical preparation and smoke gates;
- report merging;
- scored run IDs;
- calibration;
- audit;
- prompt verification permissions.

One-shot artifacts are excluded from canonical summary and audit calculations.

## Test Strategy

Test-first coverage:

1. CLI help documents new flags.
2. One-shot rejects matrices with more than one run.
3. One-shot rejects incompatible workflow flags.
4. Dry-run performs no snapshot refresh or Codex invocation.
5. Vanilla one-shot bypasses GCAL preparation.
6. Warm GCAL one-shot reuses a valid snapshot.
7. Warm GCAL one-shot refreshes a stale snapshot without spawning Codex.
8. Snapshot refresh failure produces zero model invocations.
9. Prompt places verification prohibition after task text.
10. Verification command classification covers supported build/test tools.
11. GCAL commands are never classified or limited.
12. Exactly one Codex invocation and one grader invocation occur.
13. Model or grader failure produces no retry.
14. Attempt IDs and default output paths are unique.
15. One-shot report does not merge with canonical report.
16. Existing benchmark suite remains green.

## Acceptance Criteria

- One Level 0 GCAL Luna one-shot command produces exactly one Codex invocation.
- Missing/stale snapshot refresh occurs automatically without Codex.
- Agent receives explicit no-build/no-test instruction.
- Held-out grader runs once.
- GCAL calls remain unlimited.
- Repeating the same command creates a new artifact root without duplicate IDs.
- Canonical benchmark workflows and reports remain unchanged.
