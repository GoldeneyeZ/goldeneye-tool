## Task 8: Execute and audit the randomized clean 3×3 benchmark

<TASK-ID>SSRB-8</TASK-ID>

Execute exactly three vanilla and three warm ACK runs after qualification.

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/scored/**`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.md`

- [ ] **Step 1: Derive and record seed before execution**

Use SHA-256 of:

```text
spring-sensitive-value-redaction|<task_sha256>|<candidate_executable_sha256>
```

Interpret the first eight hex digits as an unsigned integer. Store seed in
an execution manifest before dry-run:

```powershell
$SelectionPath = 'target/agent-bench/spring-sensitive-value-redaction-level2/qualification-selection.json'
$Selection = Get-Content -Raw -LiteralPath $SelectionPath | ConvertFrom-Json
if (-not $Selection.qualified) { throw "Qualification selection is not qualified" }
$QualifiedConfig = $Selection.config
$ScoredReport = $Selection.scored_report
$Provenance = Get-Content -Raw -LiteralPath $Selection.provenance | ConvertFrom-Json
$SeedMaterial = "spring-sensitive-value-redaction|$($Provenance.task_sha256)|$($Provenance.candidate_executable_sha256)"
$SeedHash = [Convert]::ToHexString(
	[Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($SeedMaterial))
).ToLowerInvariant()
$FrozenSeed = [Convert]::ToUInt32($SeedHash.Substring(0, 8), 16)
@{
	selection = $SelectionPath
	config = $QualifiedConfig
	scored_report = $ScoredReport
	seed_material = $SeedMaterial
	seed_sha256 = $SeedHash
	seed = $FrozenSeed
} | ConvertTo-Json | Set-Content -LiteralPath 'target/agent-bench/spring-sensitive-value-redaction-level2/execution-manifest.json'
```

- [ ] **Step 2: Dry-run the frozen six-run matrix**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --repetitions 3 `
  --seed $FrozenSeed `
  --out $ScoredReport `
  --dry-run
```

Expected: exactly six unique randomized IDs, three per lane.

- [ ] **Step 3: Execute all six runs serially in one invocation**

Run the same command without `--dry-run`.

Expected: six raw artifact directories. Any hard provenance/snapshot/source
gate failure aborts remaining runs.

- [ ] **Step 4: Audit the report**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --out $ScoredReport `
  --audit-report
```

Expected:

```text
Audit: PASS runs=6 candidate=3 vanilla=3 violations=0
```

- [ ] **Step 5: Verify scored vanilla median qualification**

Read the persisted `report.summary` entry for `vanilla/none` and require:

```text
successes = 3
800,000 ≤ successful_input_tokens_p50 ≤ 1,200,000
successful_uncached_input_tokens_p50 ≥ 100,000
```

If the scored median misses a token gate, preserve the complete report as a
non-qualifying scored attempt. Return to the predeclared ladder; do not alter or
discard the report.

- [ ] **Step 6: Verify final invariants**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config $QualifiedConfig --verify-only
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Also verify no `node.exe`, `codex.exe`, grader, or temporary scored worktree
process/path remains.

Expected: preparation eligible, both repositories clean, pinned Spring commit,
zero matching processes, immutable snapshot hash unchanged.

- [ ] **Step 7: Publish final analysis**

Report:

- every raw value;
- median, range, sample SD, and CV%;
- correctness;
- cached and uncached tokens separately;
- ACK call/failure/discovery counts;
- patch statistics;
- qualification evidence;
- built-in and independent audit evidence;
- limitations at `n = 3`.

State whether ACK improved or regressed each metric. Do not claim statistical
significance.

- [ ] **Step 8: Commit final benchmark documentation only**

Do not commit raw target artifacts unless repository policy explicitly requires
it.

```powershell
git add -- docs/superfastpowers/plans/SSRB
git commit -m "docs: report million token Spring benchmark"
```
