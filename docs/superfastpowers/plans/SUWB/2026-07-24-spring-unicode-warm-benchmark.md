# Spring Unicode Warm Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:goal-driven-development with `goal-driven-bypass` (recommended) to execute this plan task by task. Use superfastpowers:test-driven-development for implementation tasks and superfastpowers:verification-before-completion before claiming completion.

**Goal:** Implement and execute an auditable warm-only Goldeneye+ACK benchmark for the Spring Framework `StringUtils.truncate(CharSequence, int)` Unicode surrogate-boundary task, with one vanilla comparison run and three serial candidate repetitions.

**Architecture:** Add fail-closed snapshot and timing primitives to the existing Node benchmark harness. Prepare one immutable ACK/Goldeneye ready-cache snapshot bound to one stable absolute Spring worktree path. Restore a byte-identical live copy before every scored candidate repetition. Keep all preparation outside scored time; start `wall_ms` immediately before `codex exec` process spawn and stop it on process exit. Grade every run with a held-out Spring test, preserve raw artifacts and frozen candidate provenance, then generate a descriptive comparison report.

**Plan Acronym:** SUWB

**Tech Stack:** Node.js ESM, `node:test`, PowerShell, Git worktrees, ACK CLI, Goldeneye MCP backend, Codex CLI JSONL, Java 17, Gradle Wrapper, Spring Framework 6.2.x, Markdown/JSON artifacts

---

## Fixed Inputs and Invariants

- Harness repository: `D:\Dev\IdeaProjects\goldeneye-tool`
- Spring source repository: `D:\Dev\IdeaProjects\spring-framework`
- Spring base commit: `daf955157871e4ac6f192e06b71d6cc595eb979b`
- Spring branch context: `6.2.x`
- Stable scored worktree: `D:\Dev\IdeaProjects\.gab\spring-stringutils-worktree`
- Allowed worktree root: `D:\Dev\IdeaProjects\.gab`
- Stable live cache: `D:\Dev\IdeaProjects\.gab-cache\spring-stringutils-live`
- Allowed cache root: `D:\Dev\IdeaProjects\.gab-cache`
- Immutable snapshot: `D:\Dev\IdeaProjects\goldeneye-tool\target\agent-bench\snapshots\spring-stringutils`
- Allowed snapshot root: `D:\Dev\IdeaProjects\goldeneye-tool\target\agent-bench\snapshots`
- ACK repository: `D:\Dev\IdeaProjects\agent-context-kernel`
- ACK entrypoint: `D:\Dev\IdeaProjects\agent-context-kernel\dist\main.js`
- Goldeneye binary: `D:\Dev\IdeaProjects\goldeneye-tool\target\release\goldeneye.exe`
- Java 17 home: `C:\Users\Zacha\.jdks\openjdk-17.0.2`
- Persistent Gradle home: `D:\Dev\Caches\gradle-spring-framework-6.2`
- Model: `gpt-5.6-terra`
- Reasoning: `high`
- Candidate condition: Goldeneye+ACK, warm only, three serial repetitions
- Comparison condition: vanilla, one run only because no valid cached Spring baseline exists
- No `clean`; use Gradle daemon and local build cache
- Candidate repositories and binaries stay frozen during harness implementation and scoring
- Existing unrelated dirty files in `goldeneye-tool` and `agent-context-kernel` are never staged, reverted, or modified

## Metric Contract

- `maintenance_ms`: work completed before `codex exec` spawn, including worktree reset, snapshot restore, manifest verification, candidate verification, and engine setup.
- `wall_ms`: elapsed monotonic time beginning immediately before the `codex exec` spawn call and ending when that process exits. Any ACK, Goldeneye, MCP, or other child startup performed lazily after spawn is included.
- `grader_ms`: held-out grader duration after agent exit.
- `completion_ms`: exactly `wall_ms`; never includes maintenance.
- `verified_e2e_ms`: `wall_ms + grader_ms`.
- Vanilla comparison is descriptive reuse evidence, not paired, randomized, causal, or statistically significant evidence.

## Task 1: Build immutable snapshot primitives

<TASK-ID>SUWB-1</TASK-ID>

**Files:**

- Create: `tools/agent-bench/snapshot.mjs`
- Create: `tools/agent-bench/snapshot.test.mjs`

**Step 1: Write failing containment and unsafe-entry tests**

Create tests covering:

```js
test("assertContainedPath accepts descendants and rejects roots or escapes", () => {
  assert.doesNotThrow(() =>
    assertContainedPath("D:\\bench\\cache\\live", "D:\\bench\\cache", "live cache"));
  assert.throws(
    () => assertContainedPath("D:\\bench\\cache", "D:\\bench\\cache", "live cache"),
    /must be a strict descendant/);
  assert.throws(
    () => assertContainedPath("D:\\bench\\other", "D:\\bench\\cache", "live cache"),
    /must be a strict descendant/);
});

test("createReadySnapshot rejects symbolic links and writer artifacts", async () => {
  // Build temporary live cache with normal file, then add one symlink or
  // `goldeneye.db-wal`; each form must reject snapshot creation.
});
```

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: FAIL because `snapshot.mjs` does not exist.

**Step 2: Implement strict path and quiescence checks**

Implement and export:

```js
export function assertContainedPath(candidate, allowedRoot, label) {
  const root = path.resolve(allowedRoot);
  const target = path.resolve(candidate);
  const relative = path.relative(root, target);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`${label} must be a strict descendant of ${root}: ${target}`);
  }
  return target;
}

export async function assertNoWriterArtifacts(root) {
  const forbidden = /(?:^|[\\/])(?:goldeneye\.db-(?:wal|shm)|.*\.(?:lock|lck))$/i;
  const entries = await collectRegularFiles(root, { rejectSymlinks: true });
  const hit = entries.find(({ absolute }) => forbidden.test(absolute));
  if (hit) {
    throw new Error(`snapshot source is not quiescent: ${hit.absolute}`);
  }
}
```

`collectRegularFiles()` must:

- use `lstat()`;
- reject symbolic links, junctions, devices, sockets, and non-regular files;
- traverse directories in ordinal sorted order;
- return relative paths normalized with `/`;
- never follow links.

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: containment and unsafe-entry tests PASS.

**Step 3: Write failing manifest, isolation, and tamper tests**

Add tests proving:

- snapshot creation copies bytes rather than hardlinking;
- manifest file order is stable;
- SHA-256, byte count, and file count match;
- restoring creates a live copy with the same manifest;
- changing the restored live file leaves snapshot bytes unchanged;
- changing snapshot bytes causes verification failure;
- unexpected extra live file causes verification failure;
- source and destination outside configured roots fail before deletion/copy.

Use this manifest schema:

```json
{
  "schema_version": 1,
  "project_root": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "file_count": 1,
  "byte_count": 123,
  "files": [
    {
      "path": "ack-state/goldeneye.db",
      "bytes": 123,
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  ]
}
```

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: FAIL because snapshot creation, verification, and restore APIs are absent.

**Step 4: Implement copy-only snapshot lifecycle**

Implement:

```js
export async function createReadySnapshot({
  liveCache,
  snapshotRoot,
  allowedCacheRoot,
  allowedSnapshotRoot,
  projectRoot,
  baseRef,
}) {
  const source = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  const destination = assertContainedPath(snapshotRoot, allowedSnapshotRoot, "snapshot");
  await assertNoWriterArtifacts(source);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyTree(source, destination);
  const manifest = await buildManifest(destination, { projectRoot, baseRef });
  await writeFile(
    path.join(destination, "snapshot-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );
  await verifyReadySnapshot({ snapshotRoot: destination, expected: manifest });
  return manifest;
}

export async function restoreReadySnapshot({
  snapshotRoot,
  liveCache,
  allowedCacheRoot,
  allowedSnapshotRoot,
  expectedProjectRoot,
  expectedBaseRef,
}) {
  const source = assertContainedPath(snapshotRoot, allowedSnapshotRoot, "snapshot");
  const destination = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  const manifest = await readAndVerifyManifest(source, {
    expectedProjectRoot,
    expectedBaseRef,
  });
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyTree(source, destination, { exclude: ["snapshot-manifest.json"] });
  await verifyTreeAgainstManifest(destination, manifest);
  return manifest;
}
```

Important:

- Use `copyFile()` only; never `link()`, `copy-on-write`, or directory junctions.
- Exclude `snapshot-manifest.json` from the data-file manifest.
- Verify snapshot before copying and live cache after copying.
- Treat any absent, extra, changed, or unsafe file as a hard error.
- Record `project_root` and `base_ref` and compare exact normalized values.

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: all tests PASS.

**Step 5: Commit only Task 1 files**

```powershell
git add -- tools/agent-bench/snapshot.mjs tools/agent-bench/snapshot.test.mjs
git diff --cached --check
git commit -m "bench: add immutable ready snapshot primitives"
```

Expected: commit contains only two Task 1 files.

## Task 2: Enforce executable timing boundary and snapshot-aware runner

<TASK-ID>SUWB-2</TASK-ID>

**Files:**

- Create: `tools/agent-bench/timing.mjs`
- Create: `tools/agent-bench/timing.test.mjs`
- Modify: `tools/agent-bench/core.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`

**Step 1: Write failing timing-boundary tests**

Create:

```js
test("spawn timer excludes pre-spawn maintenance and includes spawn callback work", () => {
  let now = 1_000;
  now += 400; // maintenance before timer
  const measured = spawnWithTimer(
    () => {
      now += 25; // spawn/lazy startup overhead
      return { pid: 42 };
    },
    () => now,
  );
  now += 75; // process runtime
  assert.equal(measured.child.pid, 42);
  assert.equal(measured.elapsedMs(), 100);
});

test("scoreRunDurations keeps maintenance outside completion", () => {
  assert.deepEqual(
    scoreRunDurations({ maintenanceMs: 500, wallMs: 100, graderMs: 20 }),
    {
      maintenance_ms: 500,
      wall_ms: 100,
      grader_ms: 20,
      completion_ms: 100,
      verified_e2e_ms: 120,
    },
  );
});
```

Run:

```powershell
node --test tools/agent-bench/timing.test.mjs
```

Expected: FAIL because `timing.mjs` does not exist.

**Step 2: Implement monotonic spawn timing**

Implement:

```js
import { performance } from "node:perf_hooks";

export function spawnWithTimer(
  spawnFn,
  now = performance.now.bind(performance),
) {
  const startedAt = now();
  const child = spawnFn();
  return {
    child,
    elapsedMs: () => now() - startedAt,
  };
}

export function scoreRunDurations({ maintenanceMs, wallMs, graderMs }) {
  return {
    maintenance_ms: maintenanceMs,
    wall_ms: wallMs,
    grader_ms: graderMs,
    completion_ms: wallMs,
    verified_e2e_ms: wallMs + graderMs,
  };
}
```

Integrate `spawnWithTimer()` inside `runCodex()` so the callback contains the
actual `spawn(codexCommand, codexArgs, options)` call. Stop timing only in the
child `close` handler. Do not start ACK or Goldeneye in a pre-spawn scored timer.
Any process started because Codex invokes MCP occurs after `codex exec` spawn and
is therefore included.

Run:

```powershell
node --test tools/agent-bench/timing.test.mjs
```

Expected: PASS.

**Step 3: Write failing ready-snapshot config tests**

Extend `core.test.mjs` with a config fixture containing:

```json
{
  "ready_snapshot": {
    "root": "../../target/agent-bench/snapshots/spring-stringutils",
    "worktree": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    "live_cache": "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
    "allowed_worktree_root": "D:\\Dev\\IdeaProjects\\.gab",
    "allowed_cache_root": "D:\\Dev\\IdeaProjects\\.gab-cache",
    "allowed_snapshot_root": "D:\\Dev\\IdeaProjects\\goldeneye-tool\\target\\agent-bench\\snapshots"
  }
}
```

Assert:

- relative `root` resolves against config directory;
- all other paths resolve to absolute paths;
- missing root/allowed-root fields reject configuration;
- worktree equals neither allowed worktree root nor source repository;
- live cache and snapshot are distinct strict descendants of allowed cache root.

Run:

```powershell
node --test tools/agent-bench/core.test.mjs
```

Expected: FAIL until config normalization/validation exists.

**Step 4: Add snapshot configuration normalization**

Add `normalizeReadySnapshot(config, configPath)` to `core.mjs` and return:

```js
{
  root: resolveFromConfig(configPath, ready.root),
  worktree: path.resolve(ready.worktree),
  live_cache: path.resolve(ready.live_cache),
  allowed_worktree_root: path.resolve(ready.allowed_worktree_root),
  allowed_cache_root: path.resolve(ready.allowed_cache_root),
  allowed_snapshot_root: path.resolve(ready.allowed_snapshot_root),
}
```

Call the same strict containment primitive used by snapshot deletion. Reject
misconfigured paths during config load, before filesystem mutation.

Run:

```powershell
node --test tools/agent-bench/core.test.mjs
```

Expected: PASS.

**Step 5: Write failing runner lifecycle tests**

Refactor pure lifecycle decisions into exported helpers and test:

```js
assert.deepEqual(
  resolveRunLayout({ kind: "ack", readySnapshot, runId: "candidate-1" }),
  {
    worktree: readySnapshot.worktree,
    cacheDir: readySnapshot.live_cache,
    usesReadySnapshot: true,
  },
);

assert.equal(
  shouldPrimeIndex({ kind: "ack", usesReadySnapshot: true }),
  false,
);
```

Also assert vanilla retains unique per-run worktree behavior and never restores
the ACK snapshot.

Run:

```powershell
node --test tools/agent-bench/core.test.mjs
```

Expected: FAIL until helpers exist.

**Step 6: Integrate preparation and per-run restore**

Add CLI flag:

```text
--prepare-snapshot
```

Preparation path:

1. verify source Spring repository clean and at `base_ref`;
2. validate stable worktree/cache paths;
3. remove any registered stable worktree only after exact path validation;
4. create detached worktree at `base_ref`;
5. clear/recreate live cache;
6. set isolated `ACK_HOME`, `GOLDENEYE_DB_PATH`, `CBM_CACHE_DIR`, and project root;
7. run ACK `init` once;
8. wait for initializer exit;
9. fail if any ACK/Goldeneye process launched by initializer remains or any DB WAL/SHM/lock file exists;
10. create and verify immutable snapshot;
11. write preparation metrics and provenance; exit without a scored run.

Scored ACK path:

1. start maintenance timer;
2. verify candidate fingerprints;
3. recreate the same stable detached worktree at `base_ref`;
4. restore and verify snapshot into live cache;
5. verify source worktree cleanliness and exact commit;
6. stop maintenance timer;
7. invoke `runCodex()`; its timer starts immediately before spawn;
8. grade after process exit;
9. write duration fields with `scoreRunDurations()`.

Do not call `primeIndex()` during a scored ready-snapshot run. Preserve existing
unique worktree/cache behavior for vanilla and benchmarks without
`ready_snapshot`.

Persist per run:

```json
{
  "snapshot": {
    "manifest_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    "file_count": 0,
    "byte_count": 0,
    "project_root": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
    "restore_verified": true
  },
  "durations": {
    "maintenance_ms": 0,
    "wall_ms": 0,
    "grader_ms": 0,
    "completion_ms": 0,
    "verified_e2e_ms": 0
  }
}
```

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
```

Expected: all harness unit tests PASS.

**Step 7: Commit only Task 2 files**

```powershell
git add -- tools/agent-bench/timing.mjs tools/agent-bench/timing.test.mjs tools/agent-bench/core.mjs tools/agent-bench/core.test.mjs tools/agent-bench/bin/benchmark-agent-tasks.mjs
git diff --cached --check
git commit -m "bench: restore ready snapshots before scored runs"
```

Expected: commit excludes unrelated Goldeneye source changes.

## Task 3: Add Spring task, held-out grader, and benchmark configuration

<TASK-ID>SUWB-3</TASK-ID>

**Files:**

- Create: `tools/agent-bench/tasks/spring-stringutils-unicode-truncate.md`
- Create: `tools/agent-bench/graders/spring-stringutils-unicode-truncate.ps1`
- Create: `tools/agent-bench/configs/spring-stringutils-unicode-truncate.json`
- Create: `tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1`

**Step 1: Write the task prompt**

Use this exact behavioral contract:

```markdown
Update Spring Framework `spring-core` method
`org.springframework.util.StringUtils.truncate(CharSequence, int)`.

When truncation is required, the UTF-16 prefix must never end between the high
and low surrogate of one valid surrogate pair. If `threshold` falls between
that pair, shorten the prefix by one UTF-16 code unit before appending the
existing truncation suffix.

Preserve:
- existing positive-threshold precondition and message;
- existing suffix;
- existing behavior when `length() <= threshold`;
- existing UTF-16 code-unit threshold semantics in all other cases;
- `CharSequence` support.

Add focused coverage to
`spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`.

Run:
`.\gradlew.bat :spring-core:test --tests org.springframework.util.StringUtilsTests --build-cache`

Do not run `clean`. Do not change public API or unrelated files.
```

**Step 2: Write failing grader self-tests**

The grader self-test must create controlled patches in a disposable Spring
worktree and prove:

- old implementation fails held-out surrogate-split test;
- boundary-safe implementation passes;
- implementation that changes suffix fails;
- implementation that changes threshold precondition fails;
- solution without changes to `StringUtilsTests.java` fails protocol;
- grader restores/removes its held-out file after every outcome.

Run:

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
pwsh -NoProfile -File tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1
```

Expected: FAIL until grader exists.

**Step 3: Implement held-out grader**

The grader creates:

`spring-core/src/test/java/org/springframework/util/AgentBenchStringUtilsUnicodeTests.java`

with:

```java
package org.springframework.util;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatIllegalArgumentException;

class AgentBenchStringUtilsUnicodeTests {

	@Test
	void truncateDoesNotSplitSurrogatePair() {
		assertThat(StringUtils.truncate("abc😀rest", 4))
				.isEqualTo("abc (truncated)...");
	}

	@Test
	void truncateKeepsCompletePairAtThreshold() {
		assertThat(StringUtils.truncate("abc😀rest", 5))
				.isEqualTo("abc😀 (truncated)...");
	}

	@Test
	void truncateSupportsOtherCharSequenceImplementations() {
		assertThat(StringUtils.truncate(new StringBuilder("abc😀rest"), 4))
				.isEqualTo("abc (truncated)...");
	}

	@Test
	void truncatePreservesUntruncatedAndPreconditionBehavior() {
		assertThat(StringUtils.truncate("abc😀", 5)).isEqualTo("abc😀");
		assertThatIllegalArgumentException()
				.isThrownBy(() -> StringUtils.truncate("abc", 0))
				.withMessage("Truncation threshold must be a positive number: 0");
	}
}
```

Before testing, require:

- only allowed Spring paths are dirty:
  - `spring-core/src/main/java/org/springframework/util/StringUtils.java`
  - `spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`
- production file changed;
- repository test file changed;
- held-out filename did not already exist.

Run:

```powershell
.\gradlew.bat :spring-core:test --tests org.springframework.util.AgentBenchStringUtilsUnicodeTests --build-cache
```

Always remove held-out source in `finally`. Then run:

```powershell
git diff --check
git status --short
```

Fail on unexpected paths, test failure, protocol failure, or cleanup failure.
Capture stdout, stderr, exit status, start/end timestamps, and duration.

**Step 4: Add frozen configuration**

Create configuration containing:

```json
{
  "name": "spring-stringutils-unicode-truncate",
  "repo": "D:\\Dev\\IdeaProjects\\spring-framework",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "model": "gpt-5.6-terra",
  "reasoning": "high",
  "repetitions": 3,
  "cache_modes": ["warm"],
  "java_home": "C:\\Users\\Zacha\\.jdks\\openjdk-17.0.2",
  "gradle_user_home": "D:\\Dev\\Caches\\gradle-spring-framework-6.2",
  "ready_snapshot": {
    "root": "../../../target/agent-bench/snapshots/spring-stringutils",
    "worktree": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    "live_cache": "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
    "allowed_worktree_root": "D:\\Dev\\IdeaProjects\\.gab",
    "allowed_cache_root": "D:\\Dev\\IdeaProjects\\.gab-cache",
    "allowed_snapshot_root": "D:\\Dev\\IdeaProjects\\goldeneye-tool\\target\\agent-bench\\snapshots"
  },
  "task": {
    "id": "spring-stringutils-unicode-truncate",
    "prompt": "../tasks/spring-stringutils-unicode-truncate.md",
    "grader": "../graders/spring-stringutils-unicode-truncate.ps1",
    "extensions": [".java"]
  },
  "engines": [
    {
      "id": "goldeneye-ack",
      "kind": "ack",
      "command": "C:\\nvm4w\\nodejs\\node.exe",
      "args": [
        "D:\\Dev\\IdeaProjects\\agent-context-kernel\\dist\\main.js"
      ],
      "backend_command": "../../../target/release/goldeneye.exe",
      "cache_modes": ["warm"]
    },
    {
      "id": "vanilla",
      "kind": "vanilla",
      "cache_modes": ["none"]
    }
  ]
}
```

Adjust property names only to match existing validated harness schema. Do not
change fixed values or semantics.

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json --dry-run
```

Expected: tests PASS; dry run prints one task, candidate three repetitions,
vanilla selectable as one override run, resolved absolute paths, and no
filesystem mutations.

**Step 5: Prime Gradle dependency/build cache outside scoring**

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
Set-Location 'D:\Dev\IdeaProjects\spring-framework'
.\gradlew.bat :spring-core:test --tests org.springframework.util.StringUtilsTests --build-cache
```

Expected: `BUILD SUCCESSFUL`. Never run `clean`.

**Step 6: Re-run grader self-test**

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
pwsh -NoProfile -File tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1
```

Expected: all negative and positive grader cases PASS; source repository returns
clean at pinned commit.

**Step 7: Commit only Task 3 files**

```powershell
git add -- tools/agent-bench/tasks/spring-stringutils-unicode-truncate.md tools/agent-bench/graders/spring-stringutils-unicode-truncate.ps1 tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1 tools/agent-bench/configs/spring-stringutils-unicode-truncate.json
git diff --cached --check
git commit -m "bench: add Spring Unicode truncate task"
```

Expected: commit contains only four Task 3 files.

## Task 4: Freeze provenance, prepare snapshot, and pass smoke gates

<TASK-ID>SUWB-4</TASK-ID>

**Files:**

- Create: `tools/agent-bench/provenance.mjs`
- Create: `tools/agent-bench/provenance.test.mjs`
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Generate: `target/agent-bench/snapshots/spring-stringutils/snapshot-manifest.json`
- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/provenance.json`
- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/preparation.json`

**Step 1: Write failing deterministic fingerprint tests**

Test a temporary Git repository with tracked modifications and untracked files.
Expected provenance:

```js
{
  repo_head: "0000000000000000000000000000000000000000",
  tracked_diff_sha256: "0000000000000000000000000000000000000000000000000000000000000000",
  untracked: [
    {
      path: "dist/main.js",
      bytes: 123,
      sha256: "0000000000000000000000000000000000000000000000000000000000000000",
    },
  ],
  selected_files: [
    {
      path: "target/release/goldeneye.exe",
      bytes: 123,
      sha256: "0000000000000000000000000000000000000000000000000000000000000000",
    },
  ],
}
```

Zero hashes above are schema-shape test fixtures. Runtime captures must contain
observed hashes and reject all-zero values.

Prove:

- ordering is deterministic;
- changing tracked diff changes fingerprint;
- changing untracked file changes fingerprint;
- changing selected binary changes fingerprint;
- pre/post comparison reports exact mismatched field.

Run:

```powershell
node --test tools/agent-bench/provenance.test.mjs
```

Expected: FAIL until implementation exists.

**Step 2: Implement provenance capture and comparison**

Capture before preparation, before every scored run, and after all runs:

Goldeneye:

- repository HEAD;
- SHA-256 of `git diff --binary`;
- SHA-256 and byte size of
  `crates/application/goldeneye-query/src/engine/search.rs`;
- SHA-256 and byte size of `target/release/goldeneye.exe`.

ACK:

- repository HEAD;
- SHA-256 of `git diff --binary`;
- sorted untracked file inventory with SHA-256 and byte size;
- package-lock/yarn/pnpm lock fingerprint, whichever is present;
- SHA-256 and byte size of `dist/main.js`.

Benchmark:

- harness commit and dirty diff fingerprint;
- task, grader, config, and prompt SHA-256;
- model and reasoning;
- Spring base commit;
- Java and Gradle versions.

Use `spawnSync()` argument arrays, never shell-concatenated untrusted paths.
Normalize line endings only for Git command output before hashing; hash files as
raw bytes.

Run:

```powershell
node --test tools/agent-bench/provenance.test.mjs
node --test tools/agent-bench/*.test.mjs
```

Expected: PASS.

**Step 3: Commit Task 4 source before capturing final fingerprints**

```powershell
git add -- tools/agent-bench/provenance.mjs tools/agent-bench/provenance.test.mjs tools/agent-bench/bin/benchmark-agent-tasks.mjs
git diff --cached --check
git commit -m "bench: freeze candidate provenance"
```

Expected: only Task 4 source staged. Recompute benchmark provenance after commit.

**Step 4: Prepare immutable snapshot**

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --engine goldeneye-ack `
  --prepare-snapshot
```

Expected:

- stable Spring worktree clean at pinned commit;
- isolated live cache initialized once;
- initializer and backend stopped before copy;
- no WAL/SHM/lock artifacts;
- immutable snapshot created with copy-only files;
- sorted manifest verified;
- preparation result written;
- no Codex process spawned;
- no scored run emitted.

**Step 5: Execute one unscored smoke run**

Restore snapshot once and run the task with an explicit smoke flag:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --engine goldeneye-ack `
  --repetitions 1 `
  --smoke
```

Smoke acceptance:

- snapshot restore and verification pass;
- Codex can invoke ACK;
- held-out grader passes;
- `wall_ms` begins at Codex spawn;
- post-run live-cache mutation does not change snapshot manifest;
- candidate fingerprints unchanged;
- Spring source repository remains clean;
- no unapproved path changes.

Smoke result is excluded from scored summary and stored under a separate
`smoke/` artifact directory.

If smoke changes candidate fingerprint, snapshot, source repository, or an
unapproved path: stop. Do not score.

**Step 6: Record gate report**

Write `preparation.json` containing:

- every gate name;
- pass/fail;
- observed and expected hashes/paths/commits;
- timestamps and durations;
- exact smoke artifact path;
- explicit `eligible_for_scoring`.

Expected: `eligible_for_scoring: true`.

## Task 5: Run one vanilla comparison and three scored candidate repetitions

<TASK-ID>SUWB-5</TASK-ID>

**Files:**

- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/vanilla/**`
- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/goldeneye-ack/**`
- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/report.json`
- Generate: `target/agent-bench/spring-stringutils-unicode-truncate/report.md`

**Step 1: Reconfirm scoring eligibility**

Before any scored run:

```powershell
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --verify-only
```

Expected:

- Spring status empty;
- Spring HEAD `daf955157871e4ac6f192e06b71d6cc595eb979b`;
- preparation eligible;
- snapshot manifest valid;
- candidate fingerprints equal frozen provenance.

**Step 2: Run vanilla exactly once**

No valid cached Spring baseline exists, so create it once:

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --engine vanilla `
  --repetitions 1
```

Expected: one vanilla artifact set with prompt, JSONL, stdout/stderr, patch,
status, patch statistics, grader output/status, metrics, and provenance.

Do not rerun vanilla unless this artifact is invalid. If invalid, preserve it,
record reason, fix benchmark-only defect, and create a new explicitly versioned
attempt.

**Step 3: Run candidate three times serially**

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --engine goldeneye-ack `
  --repetitions 3
```

For each repetition, hard-gate before Codex spawn:

- frozen candidate fingerprint matches;
- snapshot manifest matches;
- stable worktree recreated clean at pinned commit;
- restored live cache exactly matches manifest;
- project root binding equals stable worktree path;
- no writer or contamination artifact exists.

Runs must be serial. Any gate failure aborts remaining scoring.

Expected: three candidate artifact sets and unchanged immutable snapshot.

**Step 4: Verify post-run invariants**

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --verify-only
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Expected:

- candidate fingerprints unchanged;
- snapshot unchanged;
- source Spring repository clean at pinned commit;
- all run artifact directories complete.

**Step 5: Build descriptive report**

Report per run:

- correctness and grader status;
- `maintenance_ms`, `wall_ms`, `grader_ms`, `completion_ms`,
  `verified_e2e_ms`;
- total, uncached input, cached input, output, and reasoning tokens;
- tool calls, ACK calls, Goldeneye/backend calls, failed calls;
- result payload bytes and cardinality;
- command failures and classified causes;
- first discovery/search selection, ordering, failed discovery commands, and
  discovery turns;
- patch files, additions, deletions, dirty paths;
- prompt/config/task/grader/provenance/snapshot hashes.

Candidate summary:

- all three raw values;
- median and range for duration/token/count metrics;
- correctness count;
- no inferential significance claims from `n=3`.

Vanilla comparison:

- label as one cached descriptive comparison;
- show provenance and artifact path;
- do not call it paired, randomized, causal, or statistically significant;
- do not infer agent effectiveness from query latency alone.

Required limitations text:

```markdown
This benchmark contains three serial Goldeneye+ACK candidate repetitions and one
vanilla comparison run. The vanilla result is descriptive reuse evidence, not a
paired or randomized control. Reported differences do not establish causality
or statistical significance. Query latency alone is not interpreted as agent
effectiveness.
```

**Step 6: Audit report against raw artifacts**

Programmatically assert:

- report run count = four scored runs;
- candidate count = three, vanilla count = one;
- every report value traces to an existing artifact;
- candidate snapshot manifest hash identical across three runs;
- candidate fingerprints identical pre/post;
- `completion_ms === wall_ms` for every run;
- `verified_e2e_ms === wall_ms + grader_ms` within integer rounding;
- every passed run contains grader PASS and allowed dirty paths only;
- report includes limitations text.

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json `
  --audit-report
```

Expected: audit PASS.

**Step 7: Final verification**

```powershell
node --test tools/agent-bench/*.test.mjs
git diff --check
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Expected:

- harness tests PASS;
- no whitespace errors;
- unrelated pre-existing `goldeneye-tool` changes remain untouched;
- Spring source repository clean at pinned commit;
- benchmark artifacts complete and audit PASS.

## Plan Self-Review Checklist

- Approved Spring behavior and focused repository-test requirement covered.
- Java 17, persistent Gradle cache, daemon/build cache, no `clean`, and warm
  priming covered.
- Immutable copy-only snapshot, stable absolute worktree binding, quiescence,
  manifest, restore verification, contamination, and tamper gates covered.
- `wall_ms` executable boundary tested at actual spawn; lazy post-spawn startup
  included; maintenance separated.
- Goldeneye, ACK, benchmark, task, grader, prompt, config, and environment
  provenance covered before, during, and after scoring.
- One vanilla run and three serial candidate repetitions covered.
- Smoke excluded from scored summary.
- Raw artifacts and report audit covered.
- Descriptive comparison limitations and unsupported-claim prohibition covered.
- No placeholder commands, paths, code, or unresolved choices remain.
