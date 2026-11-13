# GCAL improvement goal, round 6: the failed tasks again

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-5.md`.

## Short answer

Round 5 had three failures. This round repeats those three tasks, and the task
the harness had refused, on commit `045b9bb`: eight runs, six passed.

| Task | Runs of this round | Pass | All runs of this goal | Plain shell |
|---|---|---|---|---|
| terax fuzzy feature | 91k, 93k, 112k input; 2, 2, 3 calls | 3 of 3 | 6 of 7 pass; median 93k | 3 of 3; 112k |
| goldeneye rename (dirty) | 112k **failed**, 93k, 115k; 3, 2, 3 calls | 2 of 3 | 5 of 7 pass; median 112k | 3 of 3; 116k |
| ViV-Wallet feature (dirty) | 158k; 4 calls | 1 of 1 | 3 of 3 pass; median 187k | 4 of 4; 241k |
| Spring redaction feature | 491k **failed**; 9 calls | 0 of 1 | 1 of 3 pass; median 491k | 3 of 3; 809k |

- **The terax fuzzy failure of round 5 did not repeat**: three passes, at 0.83
  of plain shell's input and one call fewer.
- **The rename failed once more, for another reason.** `gcal grep '\bas_path\b'`
  printed every use, the two in `inspection.rs` among them, and the agent
  patched the other files by hand and left those two, taking `destination`
  there for a plain path. The defect that caused the failure of round 5 is
  fixed and did not occur: in the run that used `gcal callers`, the answer had
  the row for line 153 and the run passed.
- **The Spring redaction feature failed again**, on a test the agent added:
  `DataBinderFieldAccessTests.java:187: error: no suitable constructor found for MutablePropertyValues(String,String)`.
  With GCAL this task has passed 1 of 3 runs in this goal and 1 of 2 with GCAL
  v5; plain shell passed 3 of 3. It costs 0.61 of plain shell's input and does
  not reach its pass rate.

Token cost of the round: 1,294k. Cumulative: 13,739k of the 15M budget.

## What the two failures have in common

In both, the answer the agent needed was either printed (the rename) or one
command away (the constructor of `MutablePropertyValues`), and the agent wrote
its change without it. In the one-shot setting of this benchmark nothing is
compiled or tested before grading, so such an error is final. Plain shell
agents work under the same rule and passed these two tasks in all six runs.
They also read more: 13 calls and 809k tokens on the redaction feature against
9 calls and 491k.

For the redaction feature the traces give one concrete difference between the
passing run and the two failing ones. The passing run (round 4) ran before
`--tests` was bounded, and its answers carried whole test methods of the
project, 52 to 98 kB of them. The two failing runs got locations of tests
only, and the second one wrote a test that calls a constructor that does not
exist. That is a correlation over three runs, not a measurement. It was acted
on (commit `1145c8a`): an answer that lists test locations now also prints one
short test whole, with the head of its file, once per answer; about 2 kB.

## What was changed before the run

Commit `045b9bb`.

| Seen in round 5 | Change |
|---|---|
| `gcal callers` merged lines 146 and 153 of a changed file into one row that showed the declaration (goldeneye rename, failed) | Lines without an enclosing symbol are one row each with their own text; the declaration is left out, also when it has moved; a place with two to four uses shows each line. |
| `gcal get normalization.py --tests` listed two importing test files and printed neither (PyFinance) | The importing test file is printed whole when exactly one of them has at most 300 lines. |

## Reading of the runs

| Run | Commands (from the trace) |
|---|---|
| rename a (failed) | `gcal grep '\bas_path\b' crates docs -C 1`, `gcal get RevalidatedPath.as_path --tests`, a hand patch of five files, check |
| rename b | `gcal get ... --tests; gcal grep 'as_path' crates docs -C 2`, patch, a grep with check |
| rename c | `gcal get RevalidatedPath RevalidatedPath.as_path --tests`, `gcal callers RevalidatedPath.as_path; gcal grep`, patch, grep with check |
| terax fuzzy a, b | `gcal get fuzzyScore fuzzyBest --tests`, patch, check |
| terax fuzzy c | `gcal search fuzzyScore fuzzyBest --tests` (exit 1: `search` has no `--tests`), then as above |
| ViV-Wallet | two batched `gcal get ... --tests`, one range read of a test file's first 25 lines, patch, check |
| Spring redaction (failed) | four batched `gcal get ... --tests` (31 kB, 25 kB, 43 kB, 17 kB), four shell reads of the ends and beginnings of test files, patch, check |

The four rename runs of rounds 5 and 6 patched the files by hand, the two
failures among them; the runs of rounds 2 and 4 used `gcal replace` and passed.

## Verification without a model

- `pnpm check` passes (216 tests); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,735 tokens; goldeneye-tool 12 of
  12, 2,906 tokens.
- On a clone of the rename task's repository with its overlay applied and a
  fresh index, `gcal callers RevalidatedPath.as_path` printed
  `146,153  text  pub fn as_path(&self) -> &Path {` before the fix and
  `153  text  self.as_path()` after it.

## Limits

- Three runs or fewer per task in this round.
- The change made after this round (one whole test as an example) is measured
  by the runs of round 7 only.
