# GCAL improvement goal: summary

Date: 2026-11-13. Rounds: `2026-11-13-gcal-goal-round-1.md` to `-round-7.md`.

## The goal

Make GCAL (`agent/` plus the Goldeneye backend) measurably better than plain
shell tools for a coding agent: fewer tokens, fewer tool calls, less wall time,
same or better pass rate. Work in a loop: take the weakest measured point, find
its cause in recorded agent traces, fix it, verify without a model, then spend
agent runs. At most 15M tokens of agent runs.

## Result

58 agent runs with GCAL over seven rounds, 14,624k tokens. Each run is one
Codex agent (`gpt-5.6-terra`, reasoning high, codex-cli 0.159.0) on one task of
a twelve-task matrix over nine repositories, graded by a held-out grader. The
plain-shell numbers are the 43 runs of 2026-11-11 and 2026-11-12 on the same
tasks and were not repeated.

| Task | GCAL before this goal | GCAL in this goal: pass, median input, calls, wall | Plain shell: pass, median input, calls, wall | Input ratio | Wall ratio |
|---|---|---|---|---|---|
| StringUtils feature | 111k, 3 calls | 5/5, 92k, 2, 44 s | 8/8, 86k, 2, 40 s | 1.07 | 1.10 |
| terax fuzzy feature | 187k, 5 calls | 6/7, 93k, 2, 86 s | 3/3, 112k, 3, 98 s | 0.83 | 0.88 |
| abyssal feature | 124k, 3 calls | 6/6, 132k, 3, 73 s | 3/3, 155k, 4, 82 s | 0.85 | 0.89 |
| PyFinance bug | 180k, 5 calls | 5/5, 180k, 5, 93 s | 4/4, 192k, 4, 94 s | 0.94 | 0.99 |
| ViV-Wallet feature (dirty tree) | 239k, 6 calls | 3/3, 187k, 4, 114 s | 4/4, 241k, 5.5, 140 s | 0.77 | 0.81 |
| goldeneye rename (dirty tree) | 124k, 4 calls | 5/7, 112k, 3, 66 s | 3/3, 116k, 3, 69 s | 0.97 | 0.95 |
| terax module move | 433k, 11 calls | 5/5, 192k, 4, 103 s | 2/3, 235k, 6, 138 s | 0.82 | 0.74 |
| LOD signature refactor | 650k, 12 calls | 5/5, 450k, 8, 147 s | 3/3, 495k, 10, 286 s | 0.91 | 0.51 |
| Spring redaction feature | 814k, 12 calls, 1 of 2 passed | 2/4, 507k, 9, 254 s | 3/3, 809k, 13, 277 s | 0.63 | 0.92 |
| Spring flow question | 164k, 3.5 calls | 4/4, 169k, 4, 132 s | 3/3, 375k, 9, 172 s | 0.45 | 0.77 |
| agentsview flow question (dirty tree) | 972k, 15 calls | 3/3, 468k, 9, 150 s | 3/3, 1,221k, 15, 201 s | 0.38 | 0.75 |
| cbm architecture document | 828k, 12 calls | 4/4, 754k, 15, 210 s | 3/3, 1,615k, 19, 267 s | 0.47 | 0.79 |
| **Sum of medians** | **4,826k** | **53/58, 3,336k, 68 calls, 1,471 s** | **42/43, 5,652k, 93.5 calls, 1,865 s** | **0.59** | **0.79** |

"Before this goal" is the latest measurement of each task before the goal:
the hybrid lane of 2026-11-12 where it exists (GCAL v8/v9, or v6 for the Spring
flow question and the cbm document), otherwise the `gcal`-only lane of GCAL v5.
The middle column pools every run of this goal, made on seven successive
builds; it is not a measurement of the final build alone. The two full-matrix
rounds on late builds (rounds 4 and 5, 23 runs) give 3,420k, 0.61 of plain
shell.

### What improved, and by how much

- **Input tokens: 0.59 of plain shell over the twelve tasks** (3,336k against
  5,652k); before the goal the same sum was 4,826k, 0.85. The GCAL median is
  below plain shell on 11 of 12 tasks; before the goal it was on 6.
- **Tool calls: 68 against 93.5**, lower on 9 of 12 tasks, equal on 2 (StringUtils, goldeneye rename), higher on 1 (PyFinance: 5 against 4).
- **Wall time: 0.79 of plain shell** (1,471 s against 1,865 s), lower on 11 of
  12 tasks.
- **Uncached input: 427k against 589k. Output: 51k against 67k.**
- **The gaps the goal started from:**
  - Small edits cost one call too many: the StringUtils feature takes 2 calls
    in 5 of 5 runs (before: 3 calls in 4 of 6), 92k against 86k for plain
    shell. The terax fuzzy feature went from 187k and 5 calls to 93k and 2.
  - Wide refactors cost two to three times plain shell: the terax module move
    went from 433k to 192k (plain shell 235k), with runs at 126k and 80k; the
    LOD refactor from 650k to 450k (plain shell 495k) at half the wall time,
    passing 5 of 5 where the builds before the goal passed 3 of 6.
  - The backend query cap on large indexes: fixed in two places; the lookups
    that failed on the 195 MB LOD index answer in 133 ms.
  - The merged `gcal` had never been measured: it has now run 58 times.
  - Slow first daemon start: the 65 s and 116 s were the benchmark running
    during a full build. `gcal status` through a cold daemon takes 1.0 s on a
    192 MB index. The daemon also loads the graph right after its first
    answer.

### What did not improve

- **Pass rate: 53 of 58 (91%) against 42 of 43 (98%) for plain shell.** The
  goal asked for the same or better. All five failures are in rounds 5 and 6.
  - Spring redaction feature, 2 of 4 (plain shell 3 of 3; GCAL v5: 1 of 2).
    Both failures are the agent's own code that does not compile under the
    project's build (a nullness check; a constructor that does not exist in a
    test it wrote). This is the one task where GCAL is clearly cheaper and
    clearly less reliable.
  - goldeneye rename, 5 of 7 (plain shell 3 of 3). One failure was a GCAL
    defect, since fixed: `gcal callers` hid a use behind the text of the
    declaration in a file changed since indexing. The other was the agent
    leaving two uses that `gcal grep` had printed.
  - terax fuzzy feature, 6 of 7 (plain shell 3 of 3): a type error in the
    agent's own change.
  - A parse check (`gcal check`) cannot see any of these five errors; a
    compile or type check would see them all. The benchmark's one-shot rule
    forbids builds and tests in both lanes.
- **StringUtils is still above plain shell** (92k against 86k, 44 s against
  40 s): same number of calls, a longer lane prompt, and two runs that made a
  fifth model request for no visible reason.
- **PyFinance is at parity, with one call more** (180k against 192k, 5 calls
  against 4). The last run, after the change aimed at it, took 152k and 4.
- **The spread between runs of one task is large** (cbm 550k to 1,010k,
  agentsview 353k to 675k, terax module move 80k to 232k). The medians above
  rest on 3 to 7 runs.

## What was changed

21 commits on `master`, `ce7d73a` to the commit of this document.

| Area | Change | Trace or measurement that led to it |
|---|---|---|
| Backend, queries | A single-pattern `WHERE` is applied before the binding cap, in the evaluator and the `LIMIT` fast path | `gcal get` of a Python file failed on the LOD index in every run |
| Backend, index | An anonymous JavaScript function takes the name it is assigned to; a callback is not a symbol | 13 symbols named `native` on terax, one real; `callers native` was "ambiguous" |
| `callers` | A file as target lists its importers (all spellings of the path, test mocks, the lines using what is imported); 60 rows by default; one row per line when no symbol names the place; a note when text references were cut | 6 to 8 calls spent finding importers; a rename that left a use behind |
| `get` | With a symbol and a large type: the members of the type that call the symbol. With `--tests`: small files whole, the test file whole when it holds all the tests, the head of a test file, nearest tests and an example when there are none, test files importing a file, bounded output for types and large batches with one whole example | a second call to read tests, a third to read the file again, a fourth for imports; 98 kB answers |
| `grep`, `replace`, `search` | `grep` shows every fetched match; `-g` in `replace`, repeatable in both; `search` lists files by name, caps its backend page, accepts `--tests` | cut-and-repeat greps; a JSON manifest rewritten by a replace; shell file listings; two refused commands |
| Exit status | A lookup that finds nothing exits 0 | `&&` chains cancelled by "not found" |
| Daemon | Loads the project graph after the first answer of a session | 1.2 to 3 s on the first graph query |
| Skill and lane prompt | The new lookups; `\x22`/`\x27` for quotes in PowerShell patterns; `--tests` for one to three symbols | commands lost to quoting; `--tests` on 13 names |
| Harness | Overlays apply with either line ending; a failed preparation is prepared again; three configurations pointed at another machine layout | four refusals before any model call |

## Failures and incidents, in one place

- 5 failed agent runs of 58 (above).
- 5 harness refusals before any model call: two overlays with CRLF line
  endings, one configuration path, one stale snapshot accepted after a failed
  preparation, one backend shutdown that timed out while a test suite was
  running on the same machine. The first four are fixed.
- One change of this goal made a later run worse before it was bounded:
  `--tests` on large batches (round 4, 98 kB answers).

## Not verified

- The final build as such: only 4 runs were made on the last measured commit
  (`1145c8a`), and none on the commit that adds `search --tests`.
- Whether the last change (one whole test as an example) raises the pass rate
  of the Spring redaction feature: one run, which passed.
- The plain-shell lane on today's date: its runs are one to two days older.
- The index change for JavaScript on anything but terax and the 159-language
  corpus; the importers lookup for anything but TypeScript on terax and the
  unit tests for Python.
- The `gcal`-only lane (no shell discovery) was not run in this goal.
- Why the harness's pre-prompt `gcal status` takes 7 to 10 s on large indexes
  when the same command takes 1.0 s outside it.
- Fairness note that applies to every number above: the GCAL lane may run
  `gcal check`, a parse check of changed files; plain shell has no equivalent
  under the one-shot rule.

## What I would do next

1. **A real check before the agent stops.** All five failures would be
   compile or type errors. Let `gcal check` run the project's own type checker
   or compiler on the changed files when one is cheap to run (`tsc --noEmit`,
   the build's compile task, `cargo check`), and measure with the one-shot rule relaxed
   for both lanes: plain shell would then be allowed to compile too, which is
   the comparison that matters in real use.
2. **Measure the final build properly**: three runs per task of both lanes on
   the same day (about 27M tokens for GCAL and plain shell together at
   today's costs). The pooled table above mixes seven builds.
3. **Trim the lane prompt.** It has grown to 5.2 kB and the agent now adds
   `--tests` to commands of tasks that change nothing. Say less; move the rest
   to the answers themselves, where it is only read when relevant.
4. **Stale call-graph rows in changed files.** In a dirty tree `callers` still
   prints graph rows with the line numbers of the index (`141 RevalidatedPath.as_ref call`
   with no text). Re-locate them by text, as `get` does.
5. **The Spring redaction feature specifically**: read its four traces side by
   side with the three plain-shell traces to see what the shell agents read
   that the GCAL agents did not, before changing anything else for it.
6. **The harness's 7 to 10 s daemon start**: time each step of its preparation
   once; it costs nothing in tokens.
