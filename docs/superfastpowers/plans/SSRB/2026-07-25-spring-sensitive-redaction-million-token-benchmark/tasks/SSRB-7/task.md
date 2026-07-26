## Task 7: Calibrate vanilla to the one-million-token gate

<TASK-ID>SSRB-7</TASK-ID>

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/calibration/level2-attempt1/**`
- Create on the below-floor branch of the predeclared ladder:
  `tools/agent-bench/tasks/spring-sensitive-value-redaction-level3.md`
- Create on the below-floor branch of the predeclared ladder:
  `tools/agent-bench/configs/spring-sensitive-value-redaction-level3.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/qualification-selection.json`

- [ ] **Step 1: Execute exactly one Level-2 clean vanilla calibration**

Run:

```powershell
node tools/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --engine vanilla `
  --repetitions 1 `
  --calibration `
  --calibration-id level2-attempt1
```

Expected: versioned calibration artifact, grader result, qualification object;
no scored report run.

- [ ] **Step 2: Apply the predeclared decision table**

```text
PASS + 800k–1.2M input + ≥100k uncached  -> freeze Level 2
PASS + below either floor                -> implement Level 3 exactly as spec
PASS + above 1.2M input                  -> implement Level 1 exactly as spec
grader/harness defect                    -> preserve attempt, fix defect, new versioned attempt
genuine agent correctness failure        -> preserve attempt; do not count or silently rerun
```

- [ ] **Step 3: If required, add Level-3 prompt and fixtures using TDD**

Level 3 adds only:

- composed marker annotations;
- record/Kotlin-compatible accessor metadata;
- nested container-element paths;
- custom detector composition;
- context-aware custom redaction;
- formatting, equality, hashing, serialization, and source-unwrapping leak
  checks.

Run grader contract tests before and after implementation, commit the versioned
task/config, prepare a new snapshot only if include paths change, and use
calibration ID `level3-attempt1`.

- [ ] **Step 4: Re-run verification after the qualifying calibration**

Run:

```powershell
$QualifiedConfig = 'tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json'
# On the Level-3 decision branch, assign:
# $QualifiedConfig = 'tools/agent-bench/configs/spring-sensitive-value-redaction-level3.json'
node tools/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --verify-only
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
```

Expected: eligible preparation and clean repositories.

- [ ] **Step 5: Record qualification**

Persist chosen level, attempt ID, exact tokens, grader exit, task/grader hashes,
and qualification reasons. Calibration remains excluded from scored summary.
Write the selected config and scored report paths:

```json
{
  "level": 2,
  "calibration_id": "level2-attempt1",
  "config": "tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json",
  "provenance": "target/agent-bench/spring-sensitive-value-redaction-level2/provenance.json",
  "scored_report": "target/agent-bench/spring-sensitive-value-redaction-level2/report.json",
  "qualified": true
}
```

On the Level-3 branch, write the same fields with level `3`, calibration ID
`level3-attempt1`, Level-3 config, provenance, and report paths.
