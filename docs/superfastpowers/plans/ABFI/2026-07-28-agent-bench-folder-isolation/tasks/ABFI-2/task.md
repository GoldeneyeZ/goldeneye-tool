### Task 2: Relocate benchmark entrypoints

<TASK-ID>ABFI-2</TASK-ID>

**Files:**

- Move: `tools/benchmark-agent-tasks.mjs` → `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Move: `tools/benchmark-competitors.mjs` → `tools/agent-bench/bin/benchmark-competitors.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Test: `tools/agent-bench/isolation.test.mjs`

- [ ] **Step 1: Move agent-task runner and rewrite its local imports**

Move file through `apply_patch`, preserving all content except these import rewrites:

```javascript
} from "../core.mjs";
import { evaluateDirtyPathPolicy } from "../path-policy.mjs";
import { prepareCleanSnapshot } from "../snapshot.mjs";
import { buildTimingBreakdown } from "../timing.mjs";
import {
  captureBenchmarkProvenance,
  diffBenchmarkProvenance,
} from "../provenance.mjs";
import { buildBenchmarkReport } from "../report.mjs";
import { evaluateQualification } from "../qualification.mjs";
```

Every previous `./agent-bench/<module>.mjs` import becomes `../<module>.mjs`. Preserve all other behavior; because the runner moves two additional directory levels below the repository root, update its location-derived `workspace` calculation to continue resolving the repository root.

- [ ] **Step 2: Move competitor runner unchanged**

Move through `apply_patch`:

```text
tools/benchmark-competitors.mjs
→ tools/agent-bench/bin/benchmark-competitors.mjs
```

Its imports are Node built-ins; no relative rewrite is required.

- [ ] **Step 3: Update direct test reference**

In `tools/agent-bench/core.test.mjs`, replace:

```text
tools/benchmark-agent-tasks.mjs
```

with:

```text
tools/agent-bench/bin/benchmark-agent-tasks.mjs
```

- [ ] **Step 4: Run syntax checks**

```powershell
node --check tools/agent-bench/bin/benchmark-agent-tasks.mjs
node --check tools/agent-bench/bin/benchmark-competitors.mjs
```

Expected: both exit `0`, no output.

- [ ] **Step 5: Run isolation and affected tests to verify GREEN**

```powershell
node --test tools/agent-bench/isolation.test.mjs
node --test tools/agent-bench/core.test.mjs
```

Expected: both test files pass, 0 fail.

- [ ] **Step 6: Commit relocation**

```powershell
git add -- tools/benchmark-agent-tasks.mjs tools/benchmark-competitors.mjs tools/agent-bench/bin tools/agent-bench/core.test.mjs
git commit -m "refactor(bench): isolate runtime entrypoints"
```
