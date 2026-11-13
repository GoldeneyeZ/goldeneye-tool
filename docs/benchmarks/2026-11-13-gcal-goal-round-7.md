# GCAL improvement goal, round 7: the last runs of the budget

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-6.md`.

## Short answer

Four runs on commit `1145c8a`, all four passed. They close the run budget at
14,624k of 15M tokens.

| Task | This round | All runs of this goal | Plain shell |
|---|---|---|---|
| Spring redaction feature | pass; 524k input, 8 calls, 265 s | 2 of 4 pass; median 507k | 3 of 3; 809k, 13 calls, 277 s |
| PyFinance bug | pass; 152k, 4 calls, 93 s | 5 of 5 pass; median 180k | 4 of 4; 192k, 4 calls, 94 s |
| abyssal feature | pass; 100k, 2 calls, 85 s | 6 of 6 pass; median 132k | 3 of 3; 155k, 4 calls, 82 s |
| StringUtils feature | pass; 92k, 2 calls, 44 s | 5 of 5 pass; median 92k | 8 of 8; 86k, 2 calls, 40 s |

- The Spring redaction feature passed after the change made for it (one whole
  test printed as an example when `--tests` only lists locations). One run:
  it shows the change did not prevent a pass, not that it caused one. A second
  run was not made because a run of this task has cost up to 608k tokens here
  and 728k were left.
- PyFinance took its lowest input of the goal. `gcal get normalization.py --tests`
  printed the file and the one importing test file of a readable size whole
  (10 kB), and the separate read of that test file, present in all four
  earlier runs, was not made.
- Token cost of the round: 885k.

## What was changed before the run

Commit `1145c8a`: an answer of `--tests` that lists test locations also prints
one short test whole, the longest of the first candidates, with the head of
its file without the licence header, once per answer. On Spring,
`gcal get DataBinder FieldError BeanPropertyBindingResult DirectFieldBindingResult --tests`
grew from 19.5 kB to 21.2 kB and carries an 18-line test of `DataBinder` and
the imports of its test class.

## Reading of the runs

| Run | Commands (from the trace) |
|---|---|
| Spring redaction | five batched `gcal get ... --tests` (25 kB, 30 kB, 21 kB, 7 kB, 16 kB), one `gcal get` of two ranges of test files, one shell read, two patches, check |
| PyFinance | `gcal search Price --tests` (exit 1: `search` had no `--tests`), `gcal grep`, `gcal get src/pyfinance/data/normalization.py --tests`, patch, check |
| abyssal | `gcal get SecurityConfig JwtAuthenticationFilter --tests`, patch, check |
| StringUtils | `gcal get '...StringUtils.truncate' 'StringUtilsTests.java' --tests`, patch, check |

`gcal search <names> --tests` was refused with the help text in two of the last
twelve runs. After this round it answers as `gcal get <names> --tests` does
(not measured with an agent).

## The daemon start the harness records

The harness starts the daemon before the prompt with `gcal status` and records
the time: 7 to 10 s on four of the large indexes in round 4, 1 to 2 s on the
small ones. It is not part of the wall time of a run. Measured outside the
harness on the LOD repository, with a fresh index (192 MB database, 730 files,
113,950 nodes) and the project root present: `gcal status` takes 0.28 s without
the daemon and 1.03 s through a cold daemon. The product does not explain the
harness's number; what in the harness's sequence (worktree creation, snapshot
copy, overlay) makes its first `gcal status` slower was not found.

## Verification without a model

- `pnpm check` passes (217 tests); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,735 tokens; goldeneye-tool 12 of
  12, 2,906 tokens.
- On terax, `gcal search fuzzyScore fuzzyBest --tests` prints `fuzzy.ts` and
  `fuzzy.test.ts` whole and exits 0.
