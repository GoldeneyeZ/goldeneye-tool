# GCAL improvement goal, round 4: the full twelve-task matrix on one build

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-3.md`.

## Short answer

All twelve tasks of the matrix, one run each, on the same build (commit
`9946834`). All twelve passed their graders, the Spring redaction feature among
them, which the harness had refused twice.

| Task | GCAL, this round | Plain shell (median of 3 to 8) | Input ratio | Wall ratio |
|---|---|---|---|---|
| StringUtils feature | 110k input, 2 calls, 46 s | 86k, 2 calls, 40 s | 1.28 | 1.14 |
| terax fuzzy feature | 91k, 2 calls, 98 s | 112k, 3 calls, 98 s | 0.82 | 0.99 |
| abyssal feature | 98k, 2 calls, 70 s | 155k, 4 calls, 82 s | 0.64 | 0.85 |
| PyFinance bug | 188k, 5 calls, 79 s | 192k, 4 calls, 94 s | 0.98 | 0.84 |
| ViV-Wallet feature (dirty) | 208k, 4 calls, 112 s | 241k, 5.5 calls, 140 s | 0.86 | 0.80 |
| goldeneye rename (dirty) | 167k, 4 calls, 92 s | 116k, 3 calls, 69 s | 1.44 | 1.33 |
| terax module move | 192k, 4 calls, 130 s | 235k, 6 calls, 138 s | 0.82 | 0.94 |
| LOD signature refactor | 474k, 9 calls, 123 s | 495k, 10 calls, 286 s | 0.96 | 0.43 |
| Spring redaction feature | 597k, 11 calls, 301 s | 809k, 13 calls, 277 s | 0.74 | 1.08 |
| Spring flow question | 131k, 3 calls, 155 s | 375k, 9 calls, 172 s | 0.35 | 0.90 |
| agentsview flow question (dirty) | 468k, 9 calls, 150 s | 1,221k, 15 calls, 201 s | 0.38 | 0.75 |
| cbm architecture document | 550k, 10 calls, 207 s | 1,615k, 19 calls, 267 s | 0.34 | 0.78 |
| **Sum** | **3,276k, 65 calls, 1,561 s** | **5,652k, 93.5 calls, 1,865 s** | **0.58** | **0.84** |

One GCAL run per task against a plain-shell median: each ratio is a single
observation. Pass rates: GCAL 12 of 12; plain shell 42 of 43.

- Input is lower than plain shell on 10 of 12 tasks, tool calls on 9, wall
  time on 9. Uncached input: 380k against 589k. Output: 55k against 67k.
- **The two tasks above plain shell are single-run outliers of tasks that sit
  at parity.** StringUtils made the same two commands as its 89k and 90k runs
  of round 1 and paid one more model round-trip (about 21k tokens of context).
  The goldeneye rename took four calls where its earlier runs took three and
  two (113k, 91k).
- **Changes that did what they were made for**: the terax fuzzy feature and
  the abyssal feature each took one read, a patch and the check (2 calls),
  after `--tests` started printing small files whole. Both had taken 3 or 4
  calls in every earlier run.
- **A cost this goal introduced**: on the Spring redaction feature the agent
  put `--tests` on batches of 5, 9 and 13 names. The third answer was 98 kB:
  for every name up to four test methods, a file head, or an example test,
  many of them tests of other things that call a method of the same name
  (`wrap(`, `getSource(`). The run still cost 0.74 of plain shell, but the
  three first answers (52 kB, 44 kB, 98 kB) were carried through eight more
  model calls. Bounded for round 5.

Token cost of the round: 3,331k. Cumulative: 9,043k of the 15M budget.

## What was changed before the run

Commits `9081d99` and `9946834`.

| Seen in round 3 | Change |
|---|---|
| Both terax fuzzy runs read the function's file and its test file again after `gcal get ... --tests` | `--tests` is read as the intent to change a symbol: one in a file of at most 150 lines comes with the whole file, and a test file that holds all its tests is printed whole. |
| The cbm runs listed files in shell, one listing per call | `gcal search` also lists the files whose name holds a search word. |
| `gcal get normalization.py --tests` printed nothing about tests | A bare file name is answered as a path is. |
| `gcal search ... --limit 80` failed with "page limit must be between 1 and 200, got 246" | The request to the backend is capped at 200 rows. |
| The harness accepted a stale snapshot after a failed preparation | A preparation that failed is prepared again. |

## Reading of the runs

| Task | Commands (from the trace) | What it shows |
|---|---|---|
| StringUtils | `gcal get 'StringUtils.truncate' --tests`, patch, check | 2 calls, as plain shell. |
| terax fuzzy | `gcal get fuzzyScore fuzzyBest --tests` (3.4 kB: both files whole), patch, check | One read. |
| abyssal | `gcal get SecurityConfig JwtAuthenticationFilter --tests`, patch, check | One read. |
| PyFinance | `gcal get src/pyfinance --tests` (a directory listing), `gcal grep`, `gcal get normalization.py --tests` (now lists the importing test file), `gcal get tests/test_data.py`, patch, check | The 228-line test file is read whole in every run of this task. |
| ViV-Wallet | two batched `gcal get ... --tests`, a shell call reading the first 24 lines of three class files (package and imports), patch, check | `gcal get Type` prints the class without the head of its file. |
| goldeneye rename | `gcal grep`, `gcal get RevalidatedPath.as_path --tests`, `gcal replace`, patch, a final grep with check | One call more than the earlier runs. |
| terax module move | read and grep, one shell call with the move and several `gcal replace ... -g '*.ts' -g '*.tsx'`, a repair `gcal replace`, patch, check | A defect: of two `-g` options only the last was kept, so the first replace rewrote 4 of 16 files. |
| LOD | `gcal get ... --tests` of the function and both modules, grep, read, patch, two `gcal replace -g '*.py'`, a grep, two range reads, patch, grep, check | 123 s against 286 s for plain shell. |
| Spring redaction | three batched `gcal get ... --tests` (52 kB, 44 kB, 98 kB), five smaller reads, patch, check, one more read | See above. |
| Spring flow | three batched `gcal get` (80 kB, 29 kB, 13 kB) | 3 calls against 9. |
| agentsview | nine calls, batched `gcal get` and `gcal grep` | 2 "ambiguous" answers. |
| cbm document | ten commands: one listing of the repository root in shell, two `gcal search`, five batched `gcal get`, patch, check | One shell listing where the earlier runs made two to six. |

Waste counters over the twelve runs: 0 failed commands, 0 silent failures, 2
"ambiguous" answers, 11 "not found" answers inside batches, 3 searches with no
match, 25 cut outputs (10 of them in the Spring redaction run). `gcal check`
ran in all 10 editing runs.

## Verification without a model

- `pnpm check` passes (214 tests); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,712 tokens (160 more than before:
  `search` lists file names); goldeneye-tool 12 of 12, 2,944 tokens.
- On terax at the task's base commit, `gcal get fuzzyScore fuzzyBest --tests`
  prints `fuzzy.ts` (57 lines) and `fuzzy.test.ts` (44 lines) whole in 3.4 kB.

## Limits

- One run per task in this round. The StringUtils and rename results show how
  far a single run can sit from the task's other runs.
- The time the harness records for starting the daemon before the prompt is
  still 7 to 10 s on four of the large indexes and is not explained.
- The Rust backend did not change in this round.
