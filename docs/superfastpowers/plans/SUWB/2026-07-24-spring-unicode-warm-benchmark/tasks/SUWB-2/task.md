# Task 2: Enforce executable timing boundary and snapshot-aware runner

<TASK-ID>SUWB-2</TASK-ID>

**Files:**

- Create: `tools/agent-bench/timing.mjs`
- Create: `tools/agent-bench/timing.test.mjs`
- Modify: `tools/agent-bench/core.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Modify: `tools/benchmark-agent-tasks.mjs`

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
    "allowed_cache_root": "D:\\Dev\\IdeaProjects\\.gab-cache"
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
git add -- tools/agent-bench/timing.mjs tools/agent-bench/timing.test.mjs tools/agent-bench/core.mjs tools/agent-bench/core.test.mjs tools/benchmark-agent-tasks.mjs
git diff --cached --check
git commit -m "bench: restore ready snapshots before scored runs"
```

Expected: commit excludes unrelated Goldeneye source changes.
