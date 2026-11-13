# GCAL improvement goal, round 3: the dirty-tree tasks, and a second run of four

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-2.md`.

## Short answer

Six agent runs, one per task, all six passed. A seventh task (the Spring
redaction feature) was refused by the harness before any model call, for the
second time; see "Harness failures".

| Task | GCAL, this round | Plain shell (median of 3 or 4) | Input ratio | Wall ratio |
|---|---|---|---|---|
| ViV-Wallet feature (dirty tree) | 187k input, 4 calls, 117 s | 241k, 5.5 calls, 140 s | 0.77 | 0.83 |
| agentsview flow question (dirty tree) | 353k, 6 calls, 145 s | 1,221k, 15 calls, 201 s | 0.29 | 0.72 |
| PyFinance bug | 180k, 5 calls, 85 s | 192k, 4 calls, 94 s | 0.94 | 0.91 |
| terax fuzzy feature | 168k, 3 calls, 124 s | 112k, 3 calls, 98 s | 1.50 | 1.26 |
| goldeneye rename | 91k, 2 calls, 61 s | 116k, 3 calls, 69 s | 0.78 | 0.88 |
| cbm architecture document | 703k, 14 calls, 208 s | 1,615k, 19 calls, 267 s | 0.43 | 0.78 |
| **Sum** | **1,680k, 34 calls, 740 s** | **3,497k, 49.5 calls, 869 s** | **0.48** | **0.85** |

One run per task: each cell is a single observation against a median.

- The two tasks on a dirty working tree ran for the first time since GCAL v5.
  The agentsview flow question took 353k input tokens where plain shell took
  1,221k and GCAL v5 took 972k.
- Four tasks now have two runs on builds of this goal: PyFinance 202k and 180k
  (plain shell 192k), goldeneye rename 113k and 91k (116k), cbm document 806k
  and 703k (1,615k), terax fuzzy 115k and 168k (112k).
- **The terax fuzzy feature is the one task above plain shell.** The 168k run
  made the same three calls as the 115k run; the difference is that it also
  read the project's 28 kB `TERAX.md` in its first command, as one of the
  three plain-shell runs did (244k). In both GCAL runs the agent read the
  function's file and its test file whole right after
  `gcal get fuzzyScore fuzzyBest --tests` had printed the two functions and
  their tests.
- Uncached input over the six tasks: 258k against 333k for plain shell.

Token cost of the round: 1,706k (input 1,680k, output 26k). Cumulative: 5,712k
of the 15M budget.

## What was changed before the run

Commit `0ae095b`.

| Seen in round 2 | Change |
|---|---|
| `gcal get <file> --tests` printed nothing about tests (PyFinance) | It lists the test files that import the file with the lines using what they import; for a language that imports by name, the tests of the type the file declares. |
| After `--tests` the agent read the whole test file for its imports and fixtures (PyFinance) | `--tests` prints the lines above the first test of the first test file it shows, up to 40. |
| `grep` showed all matches only up to 90 (LOD: 97 matches) | Without `--limit` it asks the backend for up to 120 and shows them all when that is every match. |
| A pattern holding both quote characters was lost to PowerShell (terax, round 1) | The skill and the lane prompt name `\x22` and `\x27`. |

## Reading of the runs

| Task | Commands (from the trace) | What it shows |
|---|---|---|
| ViV-Wallet | `git status; gcal get PaymentService PaymentController PaymentDto --tests`, a second call with one more `gcal get`, three shell reads of files the first call had printed and a `git diff`, a third with two shell reads, patch, check | On a dirty tree every block carried "note: file changed since indexing; located by text (refresh: gcal index .)", and the agent read the same files again with shell. The lines `gcal` prints there are the working tree's; the note reads as a warning. |
| agentsview | six calls, all batched `gcal get` of files, ranges and `Type.method` names plus one `gcal grep` | Plain shell needed 14 to 18 calls for the same answer. |
| PyFinance | `gcal grep`, `gcal get normalization.py --tests`, a shell listing with `gcal callers`, `gcal get test_data.py`, patch, check | The new answer for a file did not appear: the agent wrote the bare file name, which was read as a symbol. |
| terax fuzzy | `Get-Content TERAX.md; gcal get fuzzyScore fuzzyBest --tests`, shell reads of `fuzzy.ts` and `fuzzy.test.ts`, patch, check | The files were read again whole after the symbols and their tests had been printed. |
| goldeneye rename | `gcal grep`, patch with `gcal replace`, check: 2 calls | Nothing to remove. |
| cbm document | 14 commands; `gcal search Indexer Indexing Index --limit 80` failed with "page limit must be between 1 and 200, got 246"; two shell listings of directories | A defect: the search asked the backend for three times the limit plus six. |

Waste counters over the six runs: 1 failed command (the `search --limit 80`
defect), 5 cut outputs, 0 "ambiguous", 5 "not found" answers inside batches, 0
silent failures.

## Harness failures

`spring-sensitive-value-redaction-level0` did not run. The refusal of round 2
(a configuration path of another machine layout) had left a preparation report
with an error and no provenance. In this round the harness read that report,
had nothing to compare the snapshot against, accepted a snapshot built on
2026-11-12 by the previous CLI (state directory `ack-state`), and the daemon
pre-warm then found no registered project. No model was called. Fixed in
`9946834`: a preparation that failed is prepared again.

## Verification without a model

- `pnpm check` passes (213 tests); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,552 tokens; goldeneye-tool 12 of
  12, 2,946 tokens (unchanged).
- The overlay patches were checked against clones of their repositories at the
  task's base commit: with LF endings the ViV-Wallet patch applies and with
  CRLF it does not; the goldeneye patch applies both ways.

## Limits

- One run per task in this round; two runs in total for four of the tasks.
- The Rust backend did not change in this round.
- The Spring redaction feature, the most expensive editing task of the matrix
  (plain shell: 809k), has still not been measured in this goal.
