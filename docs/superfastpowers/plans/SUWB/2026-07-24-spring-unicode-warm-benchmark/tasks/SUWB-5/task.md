# Task 5: Run one vanilla comparison and three scored candidate repetitions

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
