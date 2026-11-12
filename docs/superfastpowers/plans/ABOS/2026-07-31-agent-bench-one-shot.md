# Agent Bench One-Shot Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use superfastpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a fast, standalone `--one-shot` benchmark path that executes exactly one task, one engine, one cache mode, one Codex invocation, and one grader invocation without weakening canonical benchmark qualification.

**Architecture:** Keep orchestration in `benchmark-agent-tasks.mjs`, but extract one-shot policy, validation, output resolution, prompt policy, and verification-command classification into a pure `one-shot.mjs` module. Reuse existing snapshot preparation and run execution primitives. Persist one-shot results directly to an attempt-scoped report instead of merging canonical reports.

**Tech Stack:** Node.js ESM, `node:test`, existing agent-bench JSON config/report format, Codex JSONL telemetry.

---

## Scope and invariants

- CLI: `--one-shot`, `--skip-agent-verification`, `--attempt-id <id>`.
- `--one-shot` implies `--skip-agent-verification`.
- One-shot matrix requires exactly one selected task, engine, cache mode, and repetition.
- One-shot rejects `--prepare-snapshot`, `--verify-only`, `--smoke`, `--calibration`, and `--audit-report`.
- `--attempt-id` is valid only with `--one-shot` and is sanitized with existing ID rules.
- `--dry-run --one-shot` validates and prints only; zero Codex/grader invocations.
- Vanilla one-shot bypasses GCAL snapshot preparation.
- GCAL one-shot reuses a valid snapshot; missing/stale snapshot refreshes automatically without Codex smoke.
- Snapshot refresh failures stop before Codex.
- Agent prompt ends with explicit no-build/no-test/no-lint/no-check policy when verification is skipped.
- GCAL discovery calls stay unlimited.
- Telemetry records detected agent verification calls and compliance; it does not intercept commands.
- One-shot report is standalone under `target/agent-bench/<task-id>/one-shot/<attempt-id>/report.json` unless `--out` is explicit.
- One-shot never reads or merges a canonical report.
- Canonical preparation, qualification, scored runs, report merge, and audit behavior remain unchanged.

## File responsibilities

- Create: `tools/agent-bench/one-shot.mjs` — pure one-shot rules and metadata helpers.
- Create: `tools/agent-bench/one-shot.test.mjs` — unit contract for validation, paths, prompt policy, and command classification.
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs` — CLI wiring, snapshot resolution, invocation counters, standalone persistence.
- Modify: `tools/agent-bench/core.test.mjs` — runner-level subprocess regression tests using fake Codex/grader fixtures where practical.
- Modify: `tools/agent-bench/README.md` — documented fast-run command and qualification warning.

### Task ABOS-1: Pure one-shot policy module

**Files:**

- Create: `tools/agent-bench/one-shot.mjs`
- Create: `tools/agent-bench/one-shot.test.mjs`

**Step 1: Write failing validation tests**

Cover:

```js
test("one-shot requires exactly one matrix dimension", () => { /* ... */ });
test("one-shot rejects canonical workflow flags", () => { /* ... */ });
test("attempt-id is one-shot-only", () => { /* ... */ });
```

Assert stable error messages naming the invalid dimension/flag.

**Step 2: Run focused test; verify RED**

Run:

```powershell
node --test tools/agent-bench/one-shot.test.mjs
```

Expected: failure because `one-shot.mjs` does not exist.

**Step 3: Implement minimal validation API**

Export:

```js
export function validateOneShotOptions({
  enabled,
  tasks,
  engines,
  cacheModes,
  repetitions,
  flags,
}) { /* throws Error on invalid contract */ }
```

Return normalized mode fields, including implied `skipAgentVerification`.

**Step 4: Add failing attempt/path tests**

Cover explicit sanitized ID, generated non-empty ID, default output path, explicit output override, and task/attempt path containment.

**Step 5: Implement attempt/path helpers**

Export:

```js
export function resolveOneShotAttemptId(value, now = Date.now) { /* ... */ }
export function resolveOneShotOutput({ workspace, taskId, attemptId, explicitOutput }) { /* ... */ }
```

Use `sanitizeId` and `path.resolve`/`path.join`. Never overwrite canonical output by default.

**Step 6: Add failing prompt/classifier tests**

Cover shell variants for Maven, Gradle, Cargo, npm/pnpm/yarn test/build/lint/check, direct compiler commands, and false positives such as GCAL `status`, `git status`, edits, or ordinary discovery.

**Step 7: Implement policy and classifier**

Export:

```js
export function agentVerificationPolicy() { /* final prompt block */ }
export function isAgentVerificationCommand(command) { /* conservative classifier */ }
export function analyzeAgentVerificationCalls(commandCalls) { /* call list + compliant */ }
```

Classifier records only clear build/compile/test/lint/check commands. Preserve unlimited GCAL calls.

**Step 8: Run focused test; verify GREEN**

Run:

```powershell
node --test tools/agent-bench/one-shot.test.mjs
```

Expected: all pass.

### Task ABOS-2: CLI and matrix contract

**Files:**

- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Modify: `tools/agent-bench/core.test.mjs`

**Step 1: Write failing runner contract tests**

Spawn runner with fixture config and assertions for:

- help exposes all three flags;
- multi-task/multi-engine/multi-cache/repetition > 1 fails before repository mutation;
- incompatible workflow flags fail before build/snapshot/model work;
- `--attempt-id` without `--one-shot` fails;
- `--dry-run --one-shot` exits successfully with one matrix row and no model fixture marker.

**Step 2: Run targeted tests; verify RED**

Run:

```powershell
node --test --test-name-pattern="one-shot|attempt-id" tools/agent-bench/core.test.mjs
```

Expected: new assertions fail.

**Step 3: Wire flag parsing and early validation**

After task/engine/cache/repetition resolution:

```js
const oneShot = validateOneShotOptions({ /* resolved selections + flags */ });
```

Validation must precede base-commit work, build, snapshot preparation, and any child process beyond configuration parsing.

**Step 4: Resolve isolated output and run identity**

- Resolve `attemptId`.
- Override `config.output` only for one-shot.
- Use attempt directory as `runRoot`.
- Add attempt ID to run artifact identity to prevent collisions.
- Preserve explicit `--out` exactly.

**Step 5: Verify CLI tests GREEN**

Run same targeted command. Expected: all matching tests pass.

### Task ABOS-3: Snapshot fast path

**Files:**

- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Modify: `tools/agent-bench/core.test.mjs`

**Step 1: Write failing snapshot behavior tests**

Cover:

- vanilla one-shot never reads/requires preparation;
- GCAL one-shot accepts valid frozen snapshot even when smoke qualification is absent;
- missing/stale GCAL snapshot invokes existing preparation once, without smoke/Codex;
- preparation failure yields zero model invocations;
- canonical scored run still requires `eligible_for_scoring`.

Use fixture executables/files and invocation markers. No real model call.

**Step 2: Run focused tests; verify RED**

```powershell
node --test --test-name-pattern="one-shot.*snapshot|canonical.*eligible" tools/agent-bench/core.test.mjs
```

**Step 3: Implement one-shot preparation resolver**

Add runner-local orchestration:

```js
async function resolveOneShotPreparation({ baseCommit, config, expectedCandidate, repoName, engine }) {
  // vanilla => null
  // valid GCAL snapshot => existing preparation, snapshot_refreshed=false
  // missing/stale => prepareReadySnapshot(), verifyPreparedSnapshot(), snapshot_refreshed=true
}
```

Separate snapshot structural validity from canonical smoke eligibility. Reuse existing functions; do not duplicate snapshot creation logic.

**Step 4: Keep canonical gate unchanged**

Only bypass `eligible_for_scoring` inside one-shot flow. Existing normal flow retains current error.

**Step 5: Verify focused tests GREEN**

Run targeted command. Expected: all pass.

### Task ABOS-4: Single invocation, prompt policy, and telemetry

**Files:**

- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Modify: `tools/agent-bench/core.test.mjs`

**Step 1: Write failing execution tests**

Fixture Codex emits JSONL and writes a trivial patch; fixture grader returns success. Assert:

- one selected matrix item produces one Codex launch and one grader launch;
- no retry or second pass occurs;
- prompt final block forbids agent build/compile/test/lint/check;
- GCAL command count is unrestricted and does not affect compliance;
- detected verification command makes `one_shot_compliant=false` and records command/exit status;
- no detected verification makes `one_shot_compliant=true`.

**Step 2: Run focused tests; verify RED**

```powershell
node --test --test-name-pattern="one-shot.*invocation|agent verification" tools/agent-bench/core.test.mjs tools/agent-bench/one-shot.test.mjs
```

**Step 3: Pass execution policy into prompt composition**

Change signature:

```js
composePrompt(task, cacheMode, engine, { skipAgentVerification = false } = {})
```

Append policy after task text so it has final precedence. Canonical default remains byte-for-byte behavior-compatible apart from harmless function signature.

**Step 4: Count actual invocations**

Maintain per-run counters incremented immediately before `runCodex` and `runGrader`. Failed pre-launch paths remain zero; attempted launches count once.

**Step 5: Analyze telemetry**

After Codex JSONL parsing, feed command calls to `analyzeAgentVerificationCalls`. Attach:

```json
{
  "agent_verification_calls": [],
  "one_shot_compliant": true,
  "model_invocations": 1,
  "grader_invocations": 1
}
```

Do not block or limit GCAL calls.

**Step 6: Verify focused tests GREEN**

Run targeted command. Expected: all pass.

### Task ABOS-5: Standalone report persistence

**Files:**

- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Modify: `tools/agent-bench/core.test.mjs`

**Step 1: Write failing standalone report tests**

Assert:

- output contains exactly one run;
- metadata includes `mode: "one-shot"`, attempt ID, qualified false, qualification explanation, snapshot refresh state, invocation counts, policy, and compliance;
- existing canonical report at configured output is neither read nor changed;
- repeated default attempts get distinct directories;
- explicit repeated `--attempt-id` has deterministic isolated target and fails safely if it would overwrite an existing completed attempt.

**Step 2: Run focused tests; verify RED**

```powershell
node --test --test-name-pattern="one-shot.*report|canonical report" tools/agent-bench/core.test.mjs
```

**Step 3: Add direct one-shot persistence branch**

Construct fresh report only:

```js
const report = createOneShotReport({ /* config, run, preparation, counters */ });
persistReport(config.output, report);
```

Do not call `mergeReportRuns`, `persistBenchmarkReport`, or canonical audit rendering for one-shot.

**Step 4: Protect attempt artifacts**

Before execution, reject existing completed one-shot output for same explicit attempt ID. Generated IDs avoid collision by construction.

**Step 5: Verify focused tests GREEN**

Run targeted command. Expected: all pass.

### Task ABOS-6: Documentation and regression verification

**Files:**

- Modify: `tools/agent-bench/README.md`

**Step 1: Document command**

Include a copy/paste example:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level0.json `
  --one-shot --task <task-id> --engine <engine-id> --cache-modes warm `
  --repetitions 1 --model gpt-5.6-luna
```

State: standalone, unqualified, skips agent-side verification, GCAL calls unlimited, auto-refresh may still perform local snapshot initialization, and canonical scored reports are untouched.

**Step 2: Run all bench unit/integration tests**

```powershell
node --test tools/agent-bench/*.test.mjs
```

Expected: zero failures.

**Step 3: Run syntax and whitespace checks**

```powershell
node --check tools/agent-bench/one-shot.mjs
node --check tools/agent-bench/bin/benchmark-agent-tasks.mjs
git diff --check
```

Expected: zero errors.

**Step 4: Regression audit**

Compare implementation against every invariant in this plan and `docs/superfastpowers/specs/2026-07-30-agent-bench-one-shot-design.md`. Confirm:

- canonical paths unchanged;
- no GCAL call cap added;
- no real benchmark/model run occurred during tests;
- pre-existing daemon/timing changes remain intact;
- dirty unrelated files were not overwritten.

**Step 5: Record completion**

Update working plan statuses and report exact verification results. Do not commit unrelated dirty files unless explicitly requested.
