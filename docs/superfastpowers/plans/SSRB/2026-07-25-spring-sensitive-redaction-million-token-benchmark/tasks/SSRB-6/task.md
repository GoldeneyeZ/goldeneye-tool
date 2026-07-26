## Task 6: Freeze benchmark provenance

<TASK-ID>SSRB-6</TASK-ID>

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/provenance.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json`

- [ ] **Step 1: Run full harness verification**

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
git diff --check
```

Expected: all tests PASS; whitespace check empty.

- [ ] **Step 2: Build and verify frozen candidate**

Run:

```powershell
cargo test -p goldeneye-ack
cargo build --release -p goldeneye
node tools/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --verify-only
```

Expected: Rust tests PASS, release build exits `0`, preparation eligible.

- [ ] **Step 3: Verify source invariants**

Run:

```powershell
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Expected: both statuses empty; Spring HEAD
`daf955157871e4ac6f192e06b71d6cc595eb979b`.

- [ ] **Step 4: Record frozen hashes**

The preparation/provenance artifacts must include:

```json
{
  "candidate_commit": "value produced by git rev-parse HEAD",
  "candidate_executable_sha256": "value produced by provenance.mjs",
  "ack_bundle_sha256": "value produced by provenance.mjs",
  "task_sha256": "value produced by provenance.mjs",
  "grader_sha256": "value produced by provenance.mjs over grader and fixture manifest",
  "config_sha256": "value produced by provenance.mjs",
  "snapshot_manifest_sha256": "value produced by snapshot.mjs",
  "spring_commit": "daf955157871e4ac6f192e06b71d6cc595eb979b"
}
```

- [ ] **Step 5: Commit any provenance-code fix only after re-running Steps 1–4**

Expected: no uncommitted source change before calibration.
