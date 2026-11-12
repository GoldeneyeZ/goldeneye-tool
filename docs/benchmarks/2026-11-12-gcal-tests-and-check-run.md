# GCAL v8 (`get --tests`, `gcal check`) on four agent tasks, hybrid lane

Date: 2026-11-12. Follows `2026-11-12-gcal-daemon-hybrid-lanes.md`.

> Naming: when this was measured the agent CLI was a separate repository named
> ACK and was invoked as `ack`. It is now GCAL, in `agent/` of this repository,
> and this document uses the current names. Run directories on the machine that
> produced the numbers keep the old names (`m-goldeneye-ack-*`, engine
> `goldeneye-ack`).

## Question

The previous run left two places where an agent with GCAL loses to plain shell:
small edits (one call more than shell) and wide refactors (two to three times
the tokens, and failures). Three additions were built for them. Do they help
when an agent uses them, and does the agent use them?

## Short answer

- **Small edits: fixed when the new command is used, and it was used in one
  run of three.** With `gcal get StringUtils.truncate StringUtilsTests --tests`
  the StringUtils task took 2 calls and 87k input tokens, the same as plain
  shell (2 calls, 86k). The other two runs did not use it and stayed at 3 calls
  and 111k.
- **The abyssal feature is now cheaper than plain shell in every run**: 124k,
  124k, 122k against 155k (0.80).
- **The module move passes 3 of 3** (GCAL v6: 3 of 4 over both lanes) and is
  still 1.84 times dearer at the median. One run was below plain shell (214k
  against 235k).
- **The LOD refactor got worse: 0 of 3 passed, at twice the tokens of plain
  shell.** Two of the three failures come from a defect in GCAL that this run
  made visible: `gcal callers` prints a call site without its indentation, the
  agent writes its patch from that line, and the patched line gets a guessed
  indent, which in Python is a syntax error. The third failure starts with a
  second GCAL defect: a PowerShell array of paths reaches `gcal replace` as one
  argument, so the replacement "matched nothing" and the agent wrote its own
  script, which damaged an unrelated line.
- **`gcal check` would have reported the two indentation failures, and was run
  in 1 of 12 runs.** The benchmark's policy line forbids "check" commands, and
  the agents followed it over the exception that came after it.

Both GCAL defects, and the wording of the policy, were fixed after the run
(GCAL v9) and the two tasks concerned were run again:

- **With GCAL v9 the LOD refactor passes 3 of 3** (v8: 0 of 3) at a median of
  650k input tokens (v8: 997k; plain shell: 495k, so 1.31 times). All three
  runs used `gcal replace` and all six runs of the re-measurement ran
  `gcal check`.
- **The StringUtils task is unchanged**: 111k, 112k, 88k. `--tests` was again
  used in one run of three.

Summed over the four tasks (median per task): plain shell 971k input tokens,
hybrid with GCAL v6 1,371k, hybrid with GCAL v8 1,665k. Without the LOD task:
476k, 715k and 668k.

## What was built

**Goldeneye.** A new MCP tool, `check_syntax`: it parses files of an indexed
project and returns the positions of syntax errors. It takes working-tree
paths or text supplied by the caller. The files need not be indexed or
unchanged since indexing; the existing `inspect_syntax` refuses both, so it
could not be used after an edit. The tool list goes from 23 to 24.

**GCAL v8.**

| Command | What it does |
|---|---|
| `gcal get <symbol> --tests` | the symbol, then the tests that use it: the first four short ones whole, the rest as rows. A use inside a lambda is reported as the test method around it |
| `gcal check [paths]` | parses every file that differs from HEAD (or the given paths) and prints the lines that do not parse. Python files are parsed by a Python interpreter when one is installed, because a tree-sitter grammar accepts wrong indentation. A file whose committed version has the same diagnostics is not blamed on the change |
| `gcal replace` | after the changed lines: `# the pattern also matches outside these paths, unchanged (path occurrences): ...` |

**Harness.** Each run report has a `waste` block: failed commands, failed
commands that printed nothing, outputs cut at a limit, "ambiguous", "not
found" and "no match" answers, hand patches, calls of `gcal replace`,
`gcal check` and `gcal get --tests`, bytes of command output, repeated commands.

## Method

Same harness, model, prompts, graders and tasks as before. One lane was run:
hybrid (the agent may use GCAL and shell tools, with the routing rule), GCAL v8,
daemon, three runs per task on four tasks. Plain shell (three runs) and hybrid
with GCAL v6 (two runs) are reused from the earlier documents.

The prompt of the GCAL lane gained the description of the two new commands,
and one line after the one-shot policy: "Exception: gcal check is allowed. It
only parses the files you changed and runs no project code."

**This is an advantage given to one lane.** The one-shot policy forbids build,
compile, test, lint and check commands in both lanes. The GCAL lane was allowed
its own parse check; plain shell was not offered an equivalent (for example
`python -m py_compile`). Plain shell passed all 12 of its runs on these tasks
without one, so the exception could only help the GCAL lane, and in this run it
was used once.

## Result

| Task | Lane | Pass | Input, each run | Median | vs plain shell | Tool calls | Wall (median) |
|---|---|---|---|---|---|---|---|
| StringUtils feature | plain shell | 3/3 | 86k, 88k, 86k | 86k | 1.00 | 2, 2, 2 | 36 s |
| | hybrid, GCAL v6 | 2/2 | 110k, 111k | 111k | 1.28 | 3, 3 | 47 s |
| | hybrid, GCAL v8 | 3/3 | 87k, 112k, 111k | 111k | 1.29 | 2, 3, 3 | 44 s |
| abyssal feature | plain shell | 3/3 | 155k, 151k, 155k | 155k | 1.00 | 3, 4, 4 | 82 s |
| | hybrid, GCAL v6 | 2/2 | 123k, 150k | 136k | 0.88 | 3, 4 | 81 s |
| | hybrid, GCAL v8 | 3/3 | 124k, 124k, 122k | 124k | 0.80 | 3, 3, 3 | 79 s |
| terax module move | plain shell | 3/3 | 235k, 216k, 350k | 235k | 1.00 | 6, 6, 7 | 138 s |
| | hybrid, GCAL v6 | 2/2 | 309k, 628k | 468k | 2.00 | 9, 15 | 196 s |
| | hybrid, GCAL v8 | 3/3 | 476k, 433k, 214k | 433k | 1.84 | 11, 12, 7 | 181 s |
| LOD signature refactor | plain shell | 3/3 | 254k, 849k, 495k | 495k | 1.00 | 43, 10, 7 | 286 s |
| | hybrid, GCAL v6 | 1/2 | 355k, 955k F | 655k | 1.32 | 10, 10 | 321 s |
| | hybrid, GCAL v8 | 0/3 | 997k F, 697k F, 1020k F | 997k | 2.01 | 13, 12, 14 | 414 s |

Sums of the per-task medians:

| Lane | Pass | Input | Uncached input | Output | Tool calls | Wall |
|---|---|---|---|---|---|---|
| plain shell | 12/12 | 971k | 113k | 20k | 22 | 542 s |
| hybrid, GCAL v6 | 7/8 | 1,371k | 159k | 22k | 28.5 | 645 s |
| hybrid, GCAL v8 | 9/12 | 1,665k | 178k | 24k | 30 | 718 s |

Waste counters, summed over the runs of each lane (per run where a list is
given):

| Task | Lane | Failed commands (printed nothing) | Outputs cut | Ambiguous | Hand patches per run | `gcal replace` per run | `gcal check` per run | `get --tests` per run |
|---|---|---|---|---|---|---|---|---|
| StringUtils | plain shell | 0 | 0 | 0 | 1, 1, 1 | | | |
| | hybrid v6 | 0 | 0 | 0 | 1, 1 | 0, 0 | | |
| | hybrid v8 | 0 | 0 | 0 | 1, 1, 1 | 0, 0, 0 | 0, 0, 0 | 1, 0, 0 |
| abyssal | plain shell | 1 (0) | 0 | 0 | 1, 1, 1 | | | |
| | hybrid v6 | 0 | 0 | 0 | 1, 1 | 0, 0 | | |
| | hybrid v8 | 0 | 0 | 0 | 1, 1, 1 | 0, 0, 0 | 1, 0, 0 | 0, 1, 0 |
| terax move | plain shell | 3 (0) | 0 | 0 | 1, 1, 4 | | | |
| | hybrid v6 | 6 (2) | 2 | 1 | 1, 4 | 4, 0 | | |
| | hybrid v8 | 7 (3) | 6 | 4 | 3, 0, 0 | 0, 8, 4 | 0, 0, 0 | 2, 0, 0 |
| LOD refactor | plain shell | 0 | 0 | 0 | 3, 7, 6 | | | |
| | hybrid v6 | 0 | 5 | 0 | 1, 8 | 2, 0 | | |
| | hybrid v8 | 6 (1) | 6 | 0 | 7, 3, 7 | 0, 8, 0 | 0, 0, 0 | 0, 1, 0 |

## Did the agents use the new commands

| Command | Runs that used it | Reading |
|---|---|---|
| `gcal get --tests` | 4 of 12 | Where it was used on the StringUtils task, the third call disappeared |
| `gcal check` | 1 of 12 | The final message of one LOD run says "no tests, builds, linting, or parser checks": the policy line won over the exception |
| `gcal replace` | 3 of 6 refactor runs | The hybrid prompt described the command but, unlike the GCAL-only prompt, did not say to prefer it for a rename |

The cut-output counter did not fall (6 and 6 on the refactor tasks). The fix
of the previous round is that a cut output now names the files it did not
show; it does not make outputs shorter.

## Failed runs

All three are the LOD signature refactor: move a function to another module,
rename it, update 75 call sites in 40 files.

1. **Run 1.** `combine.py` line 44 and `adaptive_campaign.py` line 33:
   "unexpected indent"; 73 of 75 call sites. The agent did not use
   `gcal replace`; it wrote seven patches. The only place it saw line 33 of
   `adaptive_campaign.py` is a row of `gcal callers`:
   `33  prepare_cohort  call  tokens=len(tokenizer.encode(canonical(row['target']),...`.
   The row has no indentation. The patch replaced the line with one indented
   by 16 spaces; the file uses 8.
2. **Run 3.** The same file and line, 12 spaces instead of 8, from the same
   row. Seven patches, no `gcal replace`.
3. **Run 2.** The agent did use `gcal replace`, with the files in a PowerShell
   variable: `gcal replace '...' '...' $sourcePaths` (347 paths). Both
   replacements answered "no match". `gcal` is started through a `.ps1` script,
   and PowerShell passes an array that goes through a script as one argument,
   its elements joined by spaces (reproduced: four arguments arrive instead of
   five). The agent then replaced text with its own PowerShell script, which
   turned `object_pairs_hook=pairs` into `object_evaluation_hook=evaluation`.
   Three test files fail.

Runs 1 and 3 are the failure that this task has shown since the first
repetition (a line that loses or gains indentation). `gcal check` reports it:
checked by hand, a Python file with this error gives
`pkg/a.py` / `5: return json.dumps(x)  expected an indented block`. No failed
run called it.

Two more defects cost calls without failing a run:

- `gcal get` with six to nine file paths failed as a whole with
  `MCP tool error: unsupported Cypher query: query exceeds intermediate binding safety cap`
  (two LOD runs). One file of the batch triggers the backend limit, and the
  batch had no error isolation.
- `gcal grep ... --glob "*.py"`: "unknown option" (one run).

## Fixes made after the run (GCAL v9)

| Seen in | Before | After |
|---|---|---|
| LOD runs 1 and 3 | rows of `gcal callers`, `gcal grep` and `gcal get --tests` printed the line trimmed, with inner spaces collapsed | the line as it is in the file, indentation included |
| LOD run 2 | a path list that arrives as one argument matched nothing | an argument that is no path but splits into existing paths is that list, in `gcal grep`, `gcal replace` and `gcal check` |
| LOD runs 1 and 3 | one unanswerable item failed a whole `gcal get` batch | the item is reported (`# failed: <item>: <reason>; read it by range: ...`) and the rest is printed |
| LOD run 1 | `gcal grep --glob` unknown | `-g, --glob <glob>` as in rg |
| 11 of 12 runs | the exception for `gcal check` followed a rule that forbids "check" commands | in the GCAL lanes the rule itself is reworded: "...The one exception is gcal check... Run it once after your last edit, in the same shell call as git status, and fix what it reports." |
| 3 of 6 refactor runs | the hybrid prompt did not say when to prefer `gcal replace` | it carries the sentence of the GCAL-only prompt: use it for a rename or a signature change across many files |

The backend limit behind the failed `gcal get` batch is not fixed; only its
effect on the batch is.

## Re-measurement with GCAL v9

Hybrid lane, GCAL v9, the reworded policy, three runs on the two tasks that the
fixes concern. Plain shell and GCAL v8 are the runs above.

| Task | Lane | Pass | Input, each run | Median | vs plain shell | Uncached (median) | Tool calls | Wall (median) |
|---|---|---|---|---|---|---|---|---|
| StringUtils feature | plain shell | 3/3 | 86k, 88k, 86k | 86k | 1.00 | 14k | 2, 2, 2 | 36 s |
| | hybrid, GCAL v8 | 3/3 | 87k, 112k, 111k | 111k | 1.29 | 26k | 2, 3, 3 | 44 s |
| | hybrid, GCAL v9 | 3/3 | 111k, 112k, 88k | 111k | 1.29 | 25k | 3, 3, 2 | 42 s |
| LOD signature refactor | plain shell | 3/3 | 254k, 849k, 495k | 495k | 1.00 | 42k | 43, 10, 7 | 286 s |
| | hybrid, GCAL v8 | 0/3 | 997k F, 697k F, 1020k F | 997k | 2.01 | 74k | 13, 12, 14 | 414 s |
| | hybrid, GCAL v9 | 3/3 | 650k, 478k, 715k | 650k | 1.31 | 52k | 13, 12, 12 | 195 s |

What the agents did, per run:

| Task | `gcal replace` calls | `gcal check` calls | `get --tests` calls | Hand patches | Failed commands |
|---|---|---|---|---|---|
| StringUtils, v9 | 0, 0, 0 | 1, 1, 1 | 0, 0, 1 | 1, 1, 1 | 0, 0, 0 |
| LOD, v9 | 4, 2, 3 | 1, 1, 1 | 2, 1, 1 | 3, 1, 2 | 3, 1, 3 |
| LOD, v8 | 0, 8, 0 | 0, 0, 0 | 0, 1, 0 | 7, 3, 7 | 2, 3, 1 |

Reading:

- The reworded policy changed behaviour: `gcal check` ran in 6 of 6 runs
  (before: 1 of 12), always chained with `git status`, so it cost no extra
  call. In all three LOD runs it answered "41 changed files parse". It caught
  nothing, because nothing was broken.
- The LOD runs pass because the edit was done by `gcal replace` in all three
  (hybrid prompt now says to prefer it) and only one to three patches were
  written by hand. Whether the kept indentation in rows mattered cannot be
  told from these runs: no hand patch of a call site went wrong, and there
  were few of them.
- Three passes after a history of 3 passes in 9 GCAL-lane runs of this task are
  unlikely to be chance (about 4% if the pass rate had stayed one in three),
  but three runs do not measure a rate.
- LOD is still 1.31 times dearer than plain shell at the median, and cheaper
  than it in wall time (195 s against 286 s).
- The StringUtils task still depends on whether the agent picks `--tests`
  (one run in three in both rounds).

Defects seen in these traces, fixed after the re-measurement and not measured:

- Every LOD run began with `gcal get <symbols> --tests`, which failed as a
  whole with the backend's "intermediate binding safety cap" error (4 of the 7
  failed commands of the three runs). The lookup of the test around a match is the
  only graph query that `--tests` adds, so it is the likely cause; this was
  not confirmed on the LOD index. That lookup now falls back to the symbol the
  search reported, and a failed test lookup no longer discards the source that
  was read.
- A joined path list of absolute paths (`Get-ChildItem ... FullName`) was not
  recognised as a list (one run). It is now.

## Verification

- Goldeneye: `cargo test` for `goldeneye-mcp`, `goldeneye-services`,
  `goldeneye-compat-tests` and `goldeneye` passes; `cargo clippy --all-targets`
  on `goldeneye-services`, `goldeneye-mcp` and `goldeneye` is clean. A new
  stdio test covers an edited file, a new file, a file without grammar, a
  missing file and supplied text. The frozen `tools/list` fixture was
  regenerated; only that line changed, and the 23 existing tool definitions
  are byte-identical.
- GCAL v8 (the build that was measured): `pnpm check` passed with 190 tests.
  GCAL v9: 192 tests.
- Harness: `npm test` passes with 88 tests.
- Operation-level benchmark, GCAL calls and tokens: Spring 19/19, 20 calls,
  11,534 tokens with v8 and 11,551 with v9; goldeneye-tool 12/12, 12 calls,
  2,955 and 2,966. The increase in v9 is the indentation now kept in rows.
- By hand, through the daemon: `gcal check` on a scratch repository reports a
  Python indentation error, a TypeScript error and an error in a file created
  after indexing, and accepts a valid edit; on goldeneye-tool it parses the 77
  changed files in 1.3 s. `gcal get org.springframework.util.StringUtils.truncate --tests`
  on Spring prints both overloads and both tests in 2.9 KB.
- The PowerShell array behaviour was reproduced with a four-line script, and
  `gcal replace <pattern> <replacement> $paths` works with v9.

## Not verified

- GCAL v9 was measured on two tasks only. The abyssal feature and the terax
  module move were not rerun with it, nor any of the other eight tasks of the
  matrix.
- The claim that kept indentation removes the LOD failure is an inference from
  two traces of the v8 run; the v9 runs neither confirm nor contradict it.
- The two fixes made after the re-measurement have unit tests only.
- Three runs per task. On the terax and LOD tasks the runs of one lane differ
  by a factor of 1.5 to 2.2.
- The whole Rust workspace was not tested or linted, only the four crates
  above.
- One of five consecutive `npm test` runs of the harness had one failing test,
  while daemons of the operation-level benchmark were still alive. The test
  was not identified; the next three runs passed.
- `gcal check` compares the number of diagnostics with the committed version.
  The Python interpreter reports only the first error of a file, so a new
  error in a file that already failed to parse at HEAD is not reported.
- The Rust tool's path mode is bound to the allowed roots of the backend, like
  `inspect_syntax`. Through the GCAL daemon, which starts the backend in the
  temporary directory, it refuses every file outside it. GCAL does not use that
  mode; it sends the text.

## Cost

The 12 runs with GCAL v8 used 4.59M tokens and the 6 runs with GCAL v9 2.18M.
With the earlier runs the total is 51.7M over 114 runs.

## Reproduce

```bash
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config tools/agent-bench/configs/<task>.json --one-shot --task <task> --engine goldeneye-code-agent-layer --cache-modes warm --gcal-daemon --gcal-discovery hybrid --repetitions 1 --attempt-id m-hybrid-c1
```

The waste counters of a run are in `runs[0].waste` of its `report.json`.
