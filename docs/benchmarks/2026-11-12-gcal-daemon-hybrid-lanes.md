# GCAL v6, the GCAL daemon and a hybrid lane on six agent tasks

Date: 2026-11-12. Follows `2026-11-11-agent-task-matrix.md`. Continued in
`2026-11-12-gcal-tests-and-check-run.md`.

## Questions

1. Does the GCAL build v6 (resolver fixes, `gcal replace`) do better than v5 when
   an agent uses it?
2. Does an agent do better when it may choose between GCAL and plain shell
   tools, with a rule that says when to use which (the "hybrid" lane)?
3. How much wall time does the GCAL daemon save, compared with starting one
   Goldeneye process per `gcal` call as the benchmark did until now?

## Short answer

- **Hybrid is the cheapest lane measured so far.** Summed over the six tasks
  (median per task), hybrid used 2.36M input tokens, plain shell 2.96M, GCAL-only
  v6 2.91M and GCAL-only v5 2.70M. Hybrid is cheaper than plain shell on three
  tasks, dearer on three. It passed 11 of 12 runs; plain shell passed 18 of 18.
- **GCAL v6 is not measurably cheaper than v5** in GCAL-only mode (2.91M against
  2.70M; the spread between two runs of one task is larger than that gap). Its
  fixes are visible in the traces: "ambiguous" answers fell from 36 to 4,
  agents used `gcal replace` in all four refactor runs, and hand patches fell
  from 19 to 10. It did not fail less often: 10 of 12 passed.
- **The daemon removes the wall-time penalty.** GCAL-only took 19% more wall
  time than plain shell with one backend process per call (v5) and 1% less with
  the daemon (v6). One `gcal` call costs about 0.3 s against a warm daemon and
  about 2.0 s with a process per call on the Spring index.
- **Wide refactors are still where GCAL loses.** On the two refactor tasks the
  GCAL v6 and hybrid lanes passed 5 of 8 runs and cost 1.2 to 3 times the tokens
  of plain shell. One failure is a defect in GCAL itself: a truncated `gcal grep`
  hid five files that needed the edit.

Every lane has two runs per task (plain shell three), so a difference on one
task is an indication, not a result.

## Method

Same harness, model (`gpt-5.6-terra`, reasoning high), prompts and held-out
graders as the task matrix. Six of the twelve tasks were run: the three where
GCAL was dearer in every repetition (StringUtils feature, abyssal feature, terax
module move), the signature refactor that failed twice (LOD), and two of the
tasks where GCAL was cheaper (Spring flow question, cbm architecture document).

| Lane | GCAL build | Backend for the agent's `gcal` calls | Discovery allowed | Runs per task |
|---|---|---|---|---|
| plain shell | none | none | shell tools | 3 (matrix repetitions 1 to 3) |
| GCAL v5 | v5 | one Goldeneye process per call | `gcal` only | 2 (matrix repetitions 2 and 3) |
| GCAL v6 | v6 | GCAL daemon, started before the prompt | `gcal` only | 2 (new) |
| hybrid | v6 | GCAL daemon, started before the prompt | `gcal` and shell tools | 2 (new) |

The plain-shell and GCAL v5 numbers are reused from the task matrix; only the
last two lanes were run for this document (23 agent runs plus one smoke run
that is counted as the first hybrid run of the StringUtils task).

New harness flags, both valid only with `--one-shot`:

- `--gcal-daemon`: the agent's `gcal` calls use `GCAL_BACKEND=goldeneye` with a
  daemon under the run's isolated `GCAL_HOME`. The harness starts the daemon
  with `gcal status` before it composes the prompt and stops it after the run.
  Index preparation and snapshots still use one process per call.
- `--gcal-discovery hybrid`: the prompt allows both surfaces and gives the
  routing rule below; neither an `gcal` call nor a direct source read counts as
  a protocol violation.

The routing rule in the hybrid prompt, and now in the GCAL skill:

> Use gcal when the answer spans files: how a flow works, who calls or
> implements something, the source of several named symbols in one call, every
> use of a name across the repository, or the same edit in many files. Use
> shell tools when one known file is enough: reading or editing a file whose
> path you already have, non-source files, a quick check in one directory.

The daemon start is not part of the measured wall time. It took 1.2 to 2.2 s
on the abyssal and terax repositories and 2.7 to 12.2 s on Spring, LOD and
cbm, once per run.

## Result

Input tokens per run, the median, and the ratio of that median to the
plain-shell median. "F" marks a failed run.

| Task | Lane | Pass | Input, each run | Median | vs plain shell | Tool calls | Wall (median) |
|---|---|---|---|---|---|---|---|
| StringUtils feature | plain shell | 3/3 | 86k, 88k, 86k | 86k | 1.00 | 2, 2, 2 | 36 s |
| | GCAL v5 | 2/2 | 108k, 110k | 109k | 1.26 | 3, 3 | 47 s |
| | GCAL v6 | 2/2 | 108k, 111k | 110k | 1.27 | 3, 3 | 51 s |
| | hybrid | 2/2 | 110k, 111k | 111k | 1.28 | 3, 3 | 47 s |
| abyssal feature | plain shell | 3/3 | 155k, 151k, 155k | 155k | 1.00 | 3, 4, 4 | 82 s |
| | GCAL v5 | 2/2 | 190k, 180k | 185k | 1.20 | 4, 5 | 96 s |
| | GCAL v6 | 2/2 | 186k, 145k | 165k | 1.07 | 5, 4 | 81 s |
| | hybrid | 2/2 | 123k, 150k | 136k | 0.88 | 3, 4 | 81 s |
| terax module move | plain shell | 3/3 | 235k, 216k, 350k | 235k | 1.00 | 6, 6, 7 | 138 s |
| | GCAL v5 | 2/2 | 574k, 602k | 588k | 2.50 | 12, 12 | 270 s |
| | GCAL v6 | 1/2 | 569k, 834k F | 701k | 2.99 | 16, 21 | 252 s |
| | hybrid | 2/2 | 309k, 628k | 468k | 2.00 | 9, 15 | 196 s |
| LOD signature refactor | plain shell | 3/3 | 254k, 849k, 495k | 495k | 1.00 | 43, 10, 7 | 286 s |
| | GCAL v5 | 1/2 | 711k, 520k F | 615k | 1.24 | 13, 8 | 353 s |
| | GCAL v6 | 1/2 | 652k, 532k F | 592k | 1.19 | 16, 14 | 213 s |
| | hybrid | 1/2 | 355k, 955k F | 655k | 1.32 | 10, 10 | 321 s |
| Spring flow question | plain shell | 3/3 | 415k, 375k, 373k | 375k | 1.00 | 9, 10, 9 | 172 s |
| | GCAL v5 | 2/2 | 215k, 134k | 174k | 0.47 | 5, 3 | 128 s |
| | GCAL v6 | 2/2 | 215k, 203k | 209k | 0.56 | 5, 5 | 126 s |
| | hybrid | 2/2 | 122k, 205k | 164k | 0.44 | 3, 4 | 123 s |
| cbm architecture document | plain shell | 3/3 | 1615k, 1528k, 1775k | 1615k | 1.00 | 18, 19, 21 | 267 s |
| | GCAL v5 | 2/2 | 1259k, 790k | 1025k | 0.63 | 24, 13 | 271 s |
| | GCAL v6 | 2/2 | 1481k, 778k | 1130k | 0.70 | 20, 15 | 245 s |
| | hybrid | 2/2 | 1032k, 624k | 828k | 0.51 | 16, 8 | 222 s |

Sums of the per-task medians:

| Lane | Pass | Input | Uncached input | Output | Tool calls | Wall |
|---|---|---|---|---|---|---|
| plain shell | 18/18 | 2,961k | 268k | 35k | 50 | 981 s |
| GCAL v5 | 11/12 | 2,696k | 302k | 37k | 52.5 | 1,167 s |
| GCAL v6 | 10/12 | 2,906k | 294k | 32k | 63.5 | 968 s |
| hybrid | 11/12 | 2,362k | 279k | 33k | 44 | 991 s |

Hybrid is 20% below plain shell on input tokens and 4% above it on uncached
input tokens, which are the ones billed at the full rate. The saving is mostly
in cached tokens, that is, in fewer turns.

Two results in the table were regraded, both because the first grading failed
for a reason unrelated to the answer:

- Spring flow question, hybrid run 1: Codex was stopped before it wrote
  `final-message.txt`, so the grader read an empty answer. The answer is in the
  event stream; graded on that text it has 14 of 14 facts. The harness now
  writes the file from the event stream when Codex has not.
- terax module move, plain shell run 1: already regraded in the task matrix
  (two test files fail at the base commit).

## What the agents did

### Hybrid: which surface they picked

| Task | Commands that call `gcal`, of all commands (run 1, run 2) | Reading |
|---|---|---|
| StringUtils feature | 2 of 3, 2 of 3 | Both reads went through `gcal`; the rule did not remove the extra call. |
| abyssal feature | 1 of 3, 1 of 4 | One `gcal get` for the two classes, then shell for the test folder and one test file. |
| terax module move | 8 of 9, 2 of 15 | Run 2 gave up on `gcal` after its first command returned nothing (see "PowerShell quoting" below) and used `rg`. |
| LOD signature refactor | 10 of 10, 10 of 10 | Run 1 used `gcal replace` and passed at 355k. Run 2 patched by hand and broke the indentation of two files. |
| Spring flow question | 3 of 3, 4 of 4 | `gcal` only. |
| cbm architecture document | 14 of 16, 7 of 8 | `gcal` almost only. |

So the rule changed behaviour mainly on the abyssal task, where it turned the
GCAL lane from dearer (1.07 to 1.20) to cheaper (0.88) than plain shell. On the
StringUtils task the agent kept using `gcal get` for a file it already had the
path of, and the extra call remained.

### GCAL v6 against v5

Counted over the 12 runs of each lane:

| | GCAL v5 | GCAL v6 | hybrid |
|---|---|---|---|
| "ambiguous" answers | 36 | 4 | 2 |
| Hand patches (`apply_patch` calls) | 19 | 10 | 20 |
| Runs of the two refactor tasks that used `gcal replace` | 0 of 4 (did not exist) | 4 of 4 | 2 of 4 |
| `gcal grep` or `gcal callers` outputs cut at the limit | 25 | 23 | 19 |

The resolver fix did what it was meant to: on the cbm task v5 answered
"ambiguous" 36 times in two runs and v6 once. That did not show up as fewer
tokens on that task (1,130k against 1,025k), because run-to-run spread on the
cbm task is larger than the effect: 1,481k and 778k in the same lane.

### Failed runs

1. **terax module move, GCAL v6 run 2.** Five files still import the old module
   path. Four of the agent's seven `gcal grep` calls, including the ones that
   covered the directories of those files, were cut at the default limit
   (`# ... 86 more matching symbols; give paths or raise --limit`), and the
   five files are in none of the outputs. The agent then passed an explicit list
   of 11 files to `gcal replace`. Plain shell saw all matches in one untruncated
   `rg -n`. This is a defect of GCAL: a truncated search hides which files
   match.
2. **LOD signature refactor, GCAL v6 run 2.** The agent moved the function out of
   `src/lod/training/pairs.py`, renamed the two uses that remain in that file,
   and did not import the function there. `gcal replace` rewrote 38 import lines
   and 79 other occurrences in two calls, and the grader objects to nothing
   else. This is a mistake of the model.
3. **LOD signature refactor, hybrid run 2.** The agent did not use
   `gcal replace`; eight hand patches left `pilot.py` and `precision_probe.py`
   unparsable and three call sites unchanged. Same failure as GCAL v5 in
   repetition 3 of the matrix.

On the LOD task the GCAL lanes have now passed 3 of 6 runs (v5, v6 and hybrid
one of two each) and plain shell 3 of 3.

### Smaller findings

Four of these are fixed in GCAL v7; see "Fixes made after the runs".

- **PowerShell quoting.** A pattern such as `"['\"]native['\"]"` ends the
  PowerShell string at `\"`. PowerShell then exits with code 1 and prints
  nothing, and no part of the command line runs. Reproduced without GCAL. It hit
  6 commands, all on the terax task: 3 in GCAL v5, 1 in GCAL v6 and 2 in hybrid
  (one of those an `rg` command), none in the plain-shell lane. An agent that
  sees an `gcal` command return nothing may conclude that GCAL is broken, as
  hybrid run 2 of the terax task did.
- **Doubled backslashes.** In two of the four v6 refactor runs the first
  `gcal replace` used `\\b` inside double quotes, matched nothing, and was
  repeated with `\b`: one wasted call each.
- **Module-level names.** `gcal callers native` on the terax repository answers
  "ambiguous: native matches 13 symbols; qualify it (Type.member)". The wanted
  symbol is a module-level constant with no owner type, so the advice cannot be
  followed; the agent tried `native.ts.native`, which is not accepted.
- **Wording.** The truncation line of `gcal grep` says "matching symbols" where
  it means matching lines.

## Daemon latency without a model

Same commands, same index, output compared byte for byte (identical on 9 of 9
commands):

| Repository | Calls | One process per call | Warm daemon | Daemon cold start (`gcal status`) |
|---|---|---|---|---|
| spring-framework | 6 | 12,154 ms (2.03 s per call) | 1,806 ms (0.30 s per call) | 1,271 ms |
| goldeneye-tool | 3 | 2,646 ms (0.88 s per call) | 804 ms (0.27 s per call) | 945 ms |

The first `gcal callers` after a daemon start on Spring took 4.8 s against
0.3 s afterwards: the daemon loads the call graph on first use.

In the agent runs the daemon start measured by the harness was longer than
these cold starts on the large indexes (7.1 to 12.2 s on Spring). The harness
restores the index snapshot just before, so the first start reads a cold file.

The 116 s first daemon start seen once on the Spring index on 2026-11-11 was
not reproduced. Cold starts measured since are between 0.9 and 12.2 s. A schema
migration is ruled out (the schema has one version). The cause is unknown.

## Decisions

- The routing rule stays in the GCAL skill and `workflow/AGENTS.md`. The copies
  installed under `$HOME/.codex` were not refreshed.
- The harness flags `--gcal-daemon` and `--gcal-discovery hybrid` stay. The
  daemon lane is the fair one for wall time, because it is how GCAL runs
  outside the benchmark.

## Fixes made after the runs (GCAL v7, not measured by an agent run)

All come from the recorded commands above.

| Seen in | Before | After |
|---|---|---|
| terax module move, failed run 1 | a cut `gcal grep` said only "86 more matching symbols"; the files that were cut were unknown | the cut output ends with every matching file that has no line shown and its number of matching lines: `# matching files not shown (path matching-lines): src/modules/explorer/FileExplorer.tsx 1, src/modules/explorer/lib/{gitStatusUtils.test.ts 1, gitStatusUtils.ts 1}, ...` |
| terax, LOD (one wasted call in 2 of 4 runs) | `\\b` typed in double quotes reached `gcal grep` and `gcal replace` as two backslashes and matched nothing | when the pattern matches nothing, it is read with single backslashes; the first output line says so |
| terax | `gcal callers native`: "ambiguous ... qualify it (Type.member)", which a module-level name cannot follow; `native.ts.native` was "not found" | `file.ext.name` resolves to the symbol of that file; `gcal callers`, `callees` and `impls` accept `path:line`; the ambiguity line names both forms |
| terax (found while checking the fix above) | `gcal callers` on a constant exited 1 with a backend error, because the call graph has no node for it | its uses are listed from the text, in files of the same language only |
| terax (found while checking) | the path `src` also matched `src-tauri/src/`, for `gcal grep` and for `gcal replace` | a path that exists from the repository root means only that path; a bare name still matches anywhere |
| terax, 6 commands | no hint about the PowerShell quoting trap | one rule in the skill: write patterns in single quotes |

The scan behind the first row runs only when the output is cut. On the Spring
index (2,614 files) a cut search of the whole repository takes 0.93 s against
0.47 s for one that is not cut.

Checked by hand on the terax checkout (read only, index in a scratch
directory): `gcal grep native src -C 1`, the search of failed run 1, now names
all five files that the run missed.

## Proposed next fixes (not implemented)

1. `gcal get Type.member TestType`: when the test type is returned as an
   outline, include the members whose names contain a requested member name.
   This is the third call of the StringUtils task.
2. Daemon: load the call graph at start instead of on the first query.
3. `gcal callers` reports at most 80 text matches and does not say when that
   limit was reached.

## Verification

- Harness: `npm test` 87 pass, `npm run check` clean, after the final-message
  fallback was added. The 23 runs were made before that change, with 86 tests
  passing.
- GCAL v6: `pnpm check` (lint, 177 tests, build) passed before the runs; the GCAL
  repository was not modified during them.
- GCAL v7: `pnpm check` passes with 183 tests. Operation-level benchmark:
  Spring 19/19 in 20 calls, 11,534 tokens (v6: 11,536); goldeneye-tool 12/12
  in 12 calls, 2,955 tokens (v6: 2,944).
- Every report of the new lanes records `gcal_daemon: true`, the discovery
  mode, the daemon start time and `daemon_shutdown_verified: true`; none has a
  protocol violation.

## Not verified

- GCAL v7 has not been used by any agent run. Whether its fixes lower the cost
  or the failure rate of the two refactor tasks is not known.
- Two runs per lane and task. The two runs of one lane differ by up to a
  factor of 2 on the cbm, terax and LOD tasks, so per-task differences between
  GCAL v5, GCAL v6 and hybrid are within noise. The lane sums and the direction
  per kind of task are the usable signal.
- GCAL v5 used one backend process per call and GCAL v6 the daemon. Their
  outputs are identical, so token counts are comparable; wall times are not a
  comparison of builds, only of backends.
- The six tasks not rerun (terax fuzzy match, pyfinance, vivwallet, goldeneye
  rename, Spring L0, agentsview question) have no v6 or hybrid measurement.
- Plain shell was not rerun; its three runs are from 2026-11-11 and
  2026-11-12 morning.
- The hybrid lane was not measured without the daemon, and the routing rule was
  not measured through the installed skill, only through the benchmark prompt.

## Cost

The 23 runs and the smoke run used 10.67M tokens (input plus output). With the
task matrix (34.2M) the total is 44.9M over 96 runs.

## Reproduce

```bash
# GCAL v6 with the daemon
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config tools/agent-bench/configs/<task>.json --one-shot --task <task> --engine goldeneye-code-agent-layer --cache-modes warm --gcal-daemon --repetitions 1 --attempt-id m-ackd-b1

# hybrid lane
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config tools/agent-bench/configs/<task>.json --one-shot --task <task> --engine goldeneye-code-agent-layer --cache-modes warm --gcal-daemon --gcal-discovery hybrid --repetitions 1 --attempt-id m-hybrid-b1
```

Reports: `target/agent-bench/<task>/one-shot/<attempt-id>/report.json`; the
trace of a run is `codex.jsonl` beside its `prompt.txt` and `patch.diff`.
