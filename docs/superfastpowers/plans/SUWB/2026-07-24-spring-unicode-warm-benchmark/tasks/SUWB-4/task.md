# Task 4: Freeze provenance, prepare snapshot, and pass smoke gates

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

GCAL:

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
  --engine goldeneye-code-agent-layer `
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
  --engine goldeneye-code-agent-layer `
  --repetitions 1 `
  --smoke
```

Smoke acceptance:

- snapshot restore and verification pass;
- Codex can invoke GCAL;
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
