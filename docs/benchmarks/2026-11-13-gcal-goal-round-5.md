# GCAL improvement goal, round 5: a second full matrix, and the first failures

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-4.md`.

## Short answer

The twelve tasks again, one run each, on commit `c98f9d5`. Eleven ran; the
harness refused one before any model call. **Three of the eleven failed their
graders**, the first failures of this goal after 35 passing runs.

| Task | GCAL, this round | Plain shell (median of 3 to 8) | Input ratio | Wall ratio |
|---|---|---|---|---|
| StringUtils feature | 110k input, 2 calls, 44 s | 86k, 2 calls, 40 s | 1.28 | 1.09 |
| terax fuzzy feature | **failed**; 91k, 2 calls, 67 s | 112k, 3 calls, 98 s | 0.81 | 0.69 |
| abyssal feature | 132k, 3 calls, 69 s | 155k, 4 calls, 82 s | 0.85 | 0.85 |
| PyFinance bug | 169k, 4 calls, 95 s | 192k, 4 calls, 94 s | 0.88 | 1.01 |
| ViV-Wallet feature (dirty) | not run (harness) | 241k, 5.5 calls, 140 s | | |
| goldeneye rename (dirty) | **failed**; 93k, 3 calls, 85 s | 116k, 3 calls, 69 s | 0.80 | 1.23 |
| terax module move | 80k, 2 calls, 63 s | 235k, 6 calls, 138 s | 0.34 | 0.45 |
| LOD signature refactor | 343k, 6 calls, 140 s | 495k, 10 calls, 286 s | 0.69 | 0.49 |
| Spring redaction feature | **failed**; 446k, 9 calls, 229 s | 809k, 13 calls, 277 s | 0.55 | 0.82 |
| Spring flow question | 208k, 5 calls, 132 s | 375k, 9 calls, 172 s | 0.55 | 0.77 |
| agentsview flow question (dirty) | 675k, 12 calls, 178 s | 1,221k, 15 calls, 201 s | 0.55 | 0.88 |
| cbm architecture document | 1,010k, 17 calls, 239 s | 1,615k, 19 calls, 267 s | 0.63 | 0.89 |
| **Sum of the eleven** | **3,356k, 65 calls, 1,340 s** | **5,411k, 88 calls, 1,725 s** | **0.62** | **0.78** |

Pass rates: GCAL 8 of 11 in this round; plain shell 38 of 39 on the same eleven
tasks. A failed run's tokens say nothing about the cost of a correct answer.

## The three failures

| Task | What the grader found | Cause |
|---|---|---|
| goldeneye rename | `path_auth.rs:153: old name as_path is still used: self.as_path()` | **A GCAL defect.** The agent renamed from `gcal callers RevalidatedPath.as_path`. On this dirty tree the text search reports lines of the changed file without an enclosing symbol, and `callers` merged them into one row, `146,153  text  pub fn as_path(&self) -> &Path {`: the text of line 146, which is the declaration itself, standing for line 153, which is the use. The three earlier runs of this task found the use with `gcal grep` and passed. |
| terax fuzzy feature | the existing tests of `fuzzyScore` and `fuzzyBest` fail with "expected null not to be null" | The agent's own error. It turned `q` and `t` from strings into `{ chars, indices }` objects and left `qi === q.length` behind, outside its patch hunk. It had read the whole 57-line file in one answer. `gcal check` parses the file and cannot see a type error; `tsc` would. |
| Spring redaction feature | `DefaultBindingErrorProcessor.java:80: error: [NullAway] passing @Nullable parameter 'rejectedValue' where @NonNull is required` | The agent's own error, found by the nullness checker of the Spring build. |

The first is fixed (commit `045b9bb`): such lines are one row each with their
own text, and the declaration is left out. Checked on the task's repository
with its overlay applied: the answer now has the row `153  text  self.as_path()`.

The other two are not caused by a wrong or missing answer from GCAL. Whether
GCAL makes them more likely cannot be told from one run each: both runs wrote
their change after fewer reads than plain shell needs (2 and 9 calls against 3
and 13). Round 6 repeats the three tasks.

## Harness

`vivwallet-feature-dirty` was refused during snapshot preparation: "GCAL
initializer required forced backend shutdown". At that moment the agent's test
suite was running in another clone on the same machine; the backend did not
exit within the harness's grace period. No model was called. Nothing else was
run on the machine during the rest of the round.

## What was changed before the run

Commit `c98f9d5`.

| Seen in round 4 | Change |
|---|---|
| `--tests` on batches of 5, 9 and 13 names returned 52 kB, 44 kB and 98 kB (Spring redaction) | For a method, only test files that name its type count. For a type, or in a batch of more than three names, the answer lists where the tests are instead of printing them. |
| `-g '*.ts' -g '*.tsx'` kept the last glob only (terax module move) | `-g` is repeatable in `grep` and `replace`; a `replace` that matches nothing under its glob names the matching files left out. |
| The agent fetched the first 24 lines of three class files for package and imports (ViV-Wallet) | With `--tests` a type in a small file comes with the whole file. |
| The 228-line test file was read whole in every PyFinance run | A test file that holds all the tests of a symbol is printed whole up to 300 lines. |
| On a dirty tree every block carried a note that read as a warning | The note says the lines are the working tree's. |

Effect visible in this round: the three first answers of the Spring redaction
run were 19 kB, 11 kB and 8 kB, and the run's input 446k (round 4: 597k). The
terax module move took 2 calls and 80k tokens, with both globs honoured and
`\x22\x27` used for the quotes in its pattern.

## Reading of the other runs

- StringUtils: the same two commands as in round 4, and again five model
  requests where the runs of round 1 made four (cached input 84k against 65k).
  No command, patch or message in the trace accounts for the fifth.
- Spring flow question and agentsview: `--tests` was used in two and four
  commands of tasks that change nothing. The lane prompt describes the flag at
  length; the agent applies it where it has no use.
- cbm document: 17 commands, 6 of them file listings in shell, against 10
  commands and one listing in round 4. Input 1,010k against 550k.

Waste counters over the eleven runs: 1 failed command (an `rg --files` that
matched nothing), 4 cut outputs (round 4: 25), 1 "ambiguous", 19 "not found"
answers inside batches.

Token cost of the round: 3,403k. Cumulative: 12,446k of the 15M budget.

## Verification without a model

- `pnpm check` passes (215 tests); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,712 tokens; goldeneye-tool 12 of
  12, 2,946 tokens.
- On terax: `gcal replace ... -g '*.ts' -g '*.tsx' --dry-run` reports both
  files that hold the pattern; with `-g '*.tsx'` alone it reports no match.

## Limits

- One run per task. The spread between this round and round 4 on the same
  build family is large: cbm 550k and 1,010k, agentsview 468k and 675k, terax
  module move 192k and 80k.
- The plain-shell runs are two days older and were not repeated.
