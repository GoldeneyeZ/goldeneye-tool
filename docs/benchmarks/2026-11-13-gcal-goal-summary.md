# GCAL improvement goal: summary

Date: 2026-11-13. Rounds: `2026-11-13-gcal-goal-round-1.md` to `-round-9.md`.

## The goal

Make GCAL (`agent/` plus the Goldeneye backend) measurably better than plain
shell tools for a coding agent: fewer tokens, fewer tool calls, less wall time,
same or better pass rate. Work in a loop: take the weakest measured point, find
its cause in recorded agent traces, fix it, verify without a model, then spend
agent runs. At most 15M tokens of agent runs.

## Result

61 agent runs with GCAL over nine rounds, 14,928k tokens. Each run is one
Codex agent (`gpt-5.6-terra`, reasoning high, codex-cli 0.159.0) on one task of
a twelve-task matrix over nine repositories, graded by a held-out grader. The
plain-shell numbers are the 43 runs of 2026-11-11 and 2026-11-12 on the same
tasks and were not repeated.

| Task | GCAL before this goal | GCAL in this goal: pass, median input, calls, wall | Plain shell: pass, median input, calls, wall | Input ratio | Wall ratio |
|---|---|---|---|---|---|
| StringUtils feature | 111k, 3 calls | 6/6, 92k, 2, 44 s | 8/8, 86k, 2, 40 s | 1.06 | 1.10 |
| terax fuzzy feature | 187k, 5 calls | 6/7, 93k, 2, 86 s | 3/3, 112k, 3, 98 s | 0.83 | 0.88 |
| abyssal feature | 124k, 3 calls | 6/6, 132k, 3, 73 s | 3/3, 155k, 4, 82 s | 0.85 | 0.89 |
| PyFinance bug | 180k, 5 calls | 5/5, 180k, 5, 93 s | 4/4, 192k, 4, 94 s | 0.94 | 0.99 |
| ViV-Wallet feature (dirty tree) | 239k, 6 calls | 3/3, 187k, 4, 114 s | 4/4, 241k, 5.5, 140 s | 0.77 | 0.81 |
| goldeneye rename (dirty tree) | 124k, 4 calls | 7/9, 112k, 3, 66 s | 3/3, 116k, 3, 69 s | 0.97 | 0.95 |
| terax module move | 433k, 11 calls | 5/5, 192k, 4, 103 s | 2/3, 235k, 6, 138 s | 0.82 | 0.74 |
| LOD signature refactor | 650k, 12 calls | 5/5, 450k, 8, 147 s | 3/3, 495k, 10, 286 s | 0.91 | 0.51 |
| Spring redaction feature | 814k, 12 calls, 1 of 2 passed | 2/4, 507k, 9, 254 s | 3/3, 809k, 13, 277 s | 0.63 | 0.92 |
| Spring flow question | 164k, 3.5 calls | 4/4, 169k, 4, 132 s | 3/3, 375k, 9, 172 s | 0.45 | 0.77 |
| agentsview flow question (dirty tree) | 972k, 15 calls | 3/3, 468k, 9, 150 s | 3/3, 1,221k, 15, 201 s | 0.38 | 0.75 |
| cbm architecture document | 828k, 12 calls | 4/4, 754k, 15, 210 s | 3/3, 1,615k, 19, 267 s | 0.47 | 0.79 |
| **Sum of medians** | **4,826k** | **56/61, 3,335k, 68 calls, 1,471 s** | **42/43, 5,652k, 93.5 calls, 1,865 s** | **0.59** | **0.79** |

"Before this goal" is the latest measurement of each task before the goal:
the hybrid lane of 2026-11-12 where it exists (GCAL v8/v9, or v6 for the Spring
flow question and the cbm document), otherwise the `gcal`-only lane of GCAL v5.
The middle column pools every run of this goal, made on nine successive
builds; it is not a measurement of the final build alone. The two full-matrix
rounds on late builds (rounds 4 and 5, 23 runs) give 3,420k, 0.61 of plain
shell.

### What improved, and by how much

- **Input tokens: 0.59 of plain shell over the twelve tasks** (3,335k against
  5,652k); before the goal the same sum was 4,826k, 0.85. The GCAL median is
  below plain shell on 11 of 12 tasks; before the goal it was on 6.
- **Tool calls: 68 against 93.5**, lower on 9 of 12 tasks, equal on 2 (StringUtils, goldeneye rename), higher on 1 (PyFinance: 5 against 4).
- **Wall time: 0.79 of plain shell** (1,471 s against 1,865 s), lower on 11 of
  12 tasks.
- **Uncached input: 434k against 589k. Output: 51k against 67k.**
- **The gaps the goal started from:**
  - Small edits cost one call too many: the StringUtils feature takes 2 calls
    in 6 of 6 runs (before: 3 calls in 4 of 6), 92k against 86k for plain
    shell. The terax fuzzy feature went from 187k and 5 calls to 93k and 2.
  - Wide refactors cost two to three times plain shell: the terax module move
    went from 433k to 192k (plain shell 235k), with runs at 126k and 80k; the
    LOD refactor from 650k to 450k (plain shell 495k) at half the wall time,
    passing 5 of 5 where the builds before the goal passed 3 of 6.
  - The backend query cap on large indexes: fixed in two places; the lookups
    that failed on the 195 MB LOD index answer in 133 ms.
  - The merged `gcal` had never been measured: it has now run 61 times.
  - Slow first daemon start: the 65 s and 116 s were the benchmark running
    during a full build. `gcal status` through a cold daemon takes 1.0 s on a
    192 MB index. The daemon also loads the graph right after its first
    answer. The 7 to 12 s the harness records for its pre-warm on large
    repositories are the benchmark's hard disk, busy after `git worktree
    add`: 1.2 s with the index restored to the NVMe disk, 29 s when the same
    sequence is replayed on the hard disk (round 9).

### What did not improve

- **Pass rate: 56 of 61 (92%) against 42 of 43 (98%) for plain shell.** The
  goal asked for the same or better, and this is not it. All five failures are
  in rounds 5 and 6; the seven runs made after the last fixes for them (rounds 7
  to 9, five tasks) all passed, which is too few runs to call the gap closed.
  - Spring redaction feature, 2 of 4 (plain shell 3 of 3; GCAL v5: 1 of 2).
    Both failures are the agent's own code that does not compile under the
    project's build (a nullness check; a constructor that does not exist in a
    test it wrote). This is the one task where GCAL is clearly cheaper and
    clearly less reliable.
  - goldeneye rename, 7 of 9 (plain shell 3 of 3). One failure was a GCAL
    defect, since fixed: `gcal callers` hid a use behind the text of the
    declaration in a file changed since indexing. The other was the agent
    leaving two uses that `gcal grep` had printed.
  - terax fuzzy feature, 6 of 7 (plain shell 3 of 3): a type error in the
    agent's own change.
  - The parse check the agents ran at the end of these runs could see none of
    the five errors. `gcal check` was then extended with what the index knows
    (round 8): replayed on the recorded patches of 47 runs, it reports three of
    the five failed changes, each with the line the grader named, and none of
    the 42 that passed. The nullness error and the type error remain invisible
    to it; a compile would see all five. An opt-in `gcal check --types`
    (TypeScript, `tsc --noEmit` against HEAD) reports the type error when the
    recorded patch is replayed, and six real type errors in a run the
    tests-only grader passed. The benchmark's one-shot rule forbids
    builds and tests in both lanes.
- **StringUtils is still above plain shell** (92k against 86k, 44 s against
  40 s): same number of calls, a longer lane prompt, and two runs that made a
  fifth model request for no visible reason.
- **PyFinance is at parity, with one call more** (180k against 192k, 5 calls
  against 4). The last run, after the change aimed at it, took 152k and 4.
- **The spread between runs of one task is large** (cbm 550k to 1,010k,
  agentsview 353k to 675k, terax module move 80k to 232k). The medians above
  rest on 3 to 7 runs.

## What was changed

36 commits on `master`, `ce7d73a` to the last one.

| Area | Change | Trace or measurement that led to it |
|---|---|---|
| Backend, queries | A single-pattern `WHERE` is applied before the binding cap, in the evaluator and the `LIMIT` fast path | `gcal get` of a Python file failed on the LOD index in every run |
| Backend, index | An anonymous JavaScript function takes the name it is assigned to; a callback is not a symbol | 13 symbols named `native` on terax, one real; `callers native` was "ambiguous" |
| Backend, index | A lambda or closure in any language is not a symbol; its calls belong to the function around it (Nix functions take the name of their binding) | a Java lambda `csrf -> csrf.disable()` indexed as a function `csrf` and reported as removed by the first version of the check; 3,102 such callables in three Spring modules |
| Backend, index | A Java `package` line is not a class | an audit of the names in seven indexes: 2,920 classes named `org` or `fr`, one per Java file, a third of the classes of the Spring index; two of five rows of a `search --label Class` answer |
| `arch` | Directories of source are opened before larger directories of documents | the operations benchmark on this repository indexed again: 299 notes took the rows, `ui/src` was not shown |
| `callers` | A file as target lists its importers (all spellings of the path, test mocks, the lines using what is imported); 60 rows by default; one row per line when no symbol names the place; a note when text references were cut | 6 to 8 calls spent finding importers; a rename that left a use behind |
| `get` | With a symbol and a large type: the members of the type that call the symbol. With `--tests`: small files whole, the test file whole when it holds all the tests, the head of a test file, nearest tests and an example when there are none, test files importing a file, bounded output for types and large batches with one whole example | a second call to read tests, a third to read the file again, a fourth for imports; 98 kB answers |
| `grep`, `replace`, `search` | `grep` shows every fetched match; `-g` in `replace`, repeatable in both; `search` lists files by name, caps its backend page, accepts `--tests` | cut-and-repeat greps; a JSON manifest rewritten by a replace; shell file listings; two refused commands |
| `check` | Reports callers that still use a symbol the change removed or renamed, imports of a deleted file, and Java constructor calls no constructor takes; with `--types`, the new type errors (TypeScript) | the five failed runs; replay of 47 recorded patches |
| Exit status | A lookup that finds nothing exits 0 | `&&` chains cancelled by "not found" |
| Daemon | Loads the project graph after the first answer of a session | 1.2 to 3 s on the first graph query |
| Skill and lane prompt | The new lookups; `\x22`/`\x27` for quotes in PowerShell patterns; `--tests` for one to three symbols | commands lost to quoting; `--tests` on 13 names |
| Harness | Overlays apply with either line ending; a failed preparation is prepared again; three configurations pointed at another machine layout; `replay-check.mjs` replays recorded patches against `gcal check` without a model | four refusals before any model call; the need to verify a check without spending runs |

## Failures and incidents, in one place

- 5 failed agent runs of 61 (above).
- 5 harness refusals before any model call: two overlays with CRLF line
  endings, one configuration path, one stale snapshot accepted after a failed
  preparation, one backend shutdown that timed out while a test suite was
  running on the same machine. The first four are fixed.
- One change of this goal made a later run worse before it was bounded:
  `--tests` on large batches (round 4, 98 kB answers).

## Not verified

- The final build as such: no run was made on the last binary (Java package
  lines out of the index). 1 run (StringUtils, passed) was made on `06f7d2a`;
  2, both of the rename task, on `f191eba`; 4 on `1145c8a` before it. The
  changes after `f191eba` (`callers` on changed files, `check --types`,
  lambdas out of the index, the layout rule) were verified by unit tests, the
  language corpus, the operations benchmark and the replay of recorded
  patches; the one run does not exercise them.
- What the index change for lambdas does to `callers` answers in agent runs:
  no recorded run read such a row.
- That an agent fixes what the extended `gcal check` reports: in the two runs
  made with it the agent left nothing behind, so nothing was reported.
- Whether the last change (one whole test as an example) raises the pass rate
  of the Spring redaction feature: one run, which passed.
- The plain-shell lane on today's date: its runs are one to two days older.
- The index change for JavaScript on anything but terax and the 159-language
  corpus; the importers lookup for anything but TypeScript on terax and the
  unit tests for Python.
- The `gcal`-only lane (no shell discovery) was not run in this goal.
- Whether the hard disk the benchmark works on shifts the comparison of wall
  times. Reads of a fresh worktree are not slowed (round 9) and the pre-warm
  that takes the contention is outside the compared time; the writes at the
  end of a run were not timed.
- Fairness note that applies to every number above: the GCAL lane may run
  `gcal check` after editing; plain shell has no equivalent under the one-shot
  rule. Until round 8 it was a parse check of changed files; since then it
  also compares the change with the call graph.

## What I would do next

1. **A real check before the agent stops.** All five failures would be
   compile or type errors; the index-based check of round 8 reaches three of
   them and `gcal check --types` a fourth, for TypeScript only. Extend
   `--types` to the other languages where a check is cheap (`cargo check`, the
   build's compile task), and measure with the one-shot rule relaxed
   for both lanes: plain shell would then be allowed to compile too, which is
   the comparison that matters in real use.
2. **Measure the final build properly**: three runs per task of both lanes on
   the same day (about 27M tokens for GCAL and plain shell together at
   today's costs). The pooled table above mixes eight builds.
3. **Trim the lane prompt.** It has grown to 5.2 kB and the agent now adds
   `--tests` to commands of tasks that change nothing. Say less; move the rest
   to the answers themselves, where it is only read when relevant.
4. **Audit the index in languages the seven repositories do not have.** The
   audit of round 9 found one Java rule that turned every `package` line into
   a class; it covered Java, Python, Rust, TypeScript and little else. The
   same three questions on a Go, a Kotlin, a C# and a C++ repository cost no
   tokens. Known and left: a JavaScript destructuring pattern is one variable
   named by the whole pattern.
5. **The Spring redaction feature specifically.** Its four GCAL traces were
   compared with the three plain-shell traces after the last run. No single
   difference explains the failures: the plain-shell agents read 189 to 227 kB
   of output in 12 to 16 calls, the GCAL agents 118 to 245 kB in 8 to 11; the
   GCAL run that passed last had seen the fewest uses of the class whose
   constructor an earlier run got wrong. What is left is a higher rate of
   small API mistakes with nothing compiled before grading. A third and fourth
   repetition per lane with the build allowed would say whether that rate
   differs between the lanes at all.
6. **Move the benchmark's worktrees and caches to the NVMe disk**, for both
   lanes on the same day. Preparation of a Spring run is 45 to 57 s of
   `git worktree add` on a hard disk, and the GCAL pre-warm after it 8 to 12 s.
   For GCAL itself: open the index without creating `-wal` and `-shm` files
   when the process only reads.
