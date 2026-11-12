## Task 5: Add Level-2 configuration and six-module snapshot

<TASK-ID>SSRB-5</TASK-ID>

**Files:**
- Create:
  `tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json`
- Test: `tools/agent-bench/core.test.mjs`
- Generate:
  `target/agent-bench/snapshots/spring-sensitive-value-redaction-level2/**`

- [ ] **Step 1: Add a failing config dry-run test**

Add a fixture config test that loads the new JSON and asserts:

```javascript
assert.deepEqual(config.qualification, {
  min_input_tokens: 800_000,
  max_input_tokens: 1_200_000,
  min_uncached_input_tokens: 100_000,
});
assert.deepEqual(config.audit, {
  expected_candidate_runs: 3,
  expected_vanilla_runs: 3,
});
assert.equal(config.allowed_dirty_policy.max_paths, 40);
```

- [ ] **Step 2: Run test and verify missing-config failure**

Run:

```powershell
node --test tools/agent-bench/core.test.mjs
```

Expected: FAIL with missing Level-2 config.

- [ ] **Step 3: Create pinned config**

Key values:

```json
{
  "name": "spring-sensitive-value-redaction-level2",
  "output": "../../../target/agent-bench/spring-sensitive-value-redaction-level2/report.json",
  "repo": "D:\\Dev\\IdeaProjects\\spring-framework",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "model": "gpt-5.6-terra",
  "reasoning": "high",
  "codex_full_access": true,
  "repetitions": 3,
  "timeout_ms": 3600000,
  "cache_modes": ["warm"],
  "qualification": {
    "min_input_tokens": 800000,
    "max_input_tokens": 1200000,
    "min_uncached_input_tokens": 100000
  },
  "audit": {
    "expected_candidate_runs": 3,
    "expected_vanilla_runs": 3
  },
  "allowed_dirty_policy": {
    "prefixes": [
      "spring-core/src/",
      "spring-beans/src/",
      "spring-context/src/",
      "spring-web/src/",
      "spring-webmvc/src/",
      "spring-webflux/src/"
    ],
    "required_prefixes": [
      "spring-core/src/main/java/",
      "spring-context/src/main/java/",
      "spring-web/src/main/java/"
    ],
    "min_paths": 8,
    "max_paths": 40
  }
}
```

Configure the task, grader, GCAL engine, vanilla engine, Java home, Gradle home,
stable worktree, live cache, and snapshot roots exactly as the prior Spring
config, using new unique paths.

Set `GOLDENEYE_INCLUDE_PATHS` to the `src/main/java` and `src/test/java` trees
of all six modules.

- [ ] **Step 4: Verify dry-run matrix**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --repetitions 3 `
  --seed 20260725 `
  --dry-run
```

Expected: six unique runs, three `goldeneye-code-agent-layer/warm` and three
`vanilla/none`.

- [ ] **Step 5: Prepare immutable snapshot**

Run:

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --engine goldeneye-code-agent-layer `
  --prepare-snapshot
```

Expected: eligible preparation, manifest SHA-256, no writer artifacts, no
Codex process.

- [ ] **Step 6: Verify preparation**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --verify-only
```

Expected: `Preparation gates: ELIGIBLE`.

- [ ] **Step 7: Run harness tests**

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: all tests PASS.

- [ ] **Step 8: Commit config**

Do not commit snapshot artifacts.

```powershell
git add -- tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json tools/agent-bench/core.test.mjs
git commit -m "bench: configure million token Spring task"
```
