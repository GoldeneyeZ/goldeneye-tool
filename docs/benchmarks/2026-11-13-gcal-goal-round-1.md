# GCAL improvement goal, round 1: the merged `gcal` measured, four gaps closed

Date: 2026-11-13. Follows `2026-11-12-gcal-tests-and-check-run.md`.

This is the first round of an eight-hour loop whose aim is to make GCAL
(`agent/` plus the Goldeneye backend) measurably better than plain shell tools
for a coding agent: fewer tokens, fewer tool calls, less wall time, same or
better pass rate. Each round takes the weakest measured point, finds its cause
in recorded agent traces, fixes it, verifies without a model, and only then
spends agent runs.

## Short answer

Ten runs (five tasks, two each) with the merged `gcal`, which had never been
measured, after four fixes. All ten passed their held-out graders.

| Task | GCAL, this round | GCAL, previous build | Plain shell | This round vs plain shell |
|---|---|---|---|---|
| StringUtils feature (one-method edit) | 89k input, 2 calls, 47 s | 111k, 3 calls, 43 s | 86k, 2 calls, 36 s | 1.04 input, 1.29 wall |
| abyssal feature | 143k, 3.5 calls, 78 s | 124k, 3 calls, 79 s | 155k, 4 calls, 82 s | 0.92 input, 0.96 wall |
| terax module move (26 files) | 179k, 4.5 calls, 92 s | 433k, 11 calls, 181 s | 235k, 6 calls, 138 s | 0.76 input, 0.67 wall |
| LOD signature refactor (40 files) | 451k, 8.5 calls, 172 s | 706k, 12.5 calls, 268 s | 495k, 10 calls, 286 s | 0.91 input, 0.60 wall |
| Spring flow question | 169k, 4.5 calls, 109 s | 164k, 3.5 calls, 123 s | 375k, 9 calls, 172 s | 0.45 input, 0.64 wall |
| **Sum of medians** | **1,031k, 23 calls, 498 s** | | **1,346k, 31 calls, 714 s** | **0.77 input, 0.70 wall** |

Values are medians of input tokens (cached input included), tool calls and
wall time. Pass rates: GCAL this round 10 of 10; plain shell 14 of 15 (one
terax run failed its grader); GCAL previous build 15 of 18 on the four tasks
it ran (three LOD failures). "Previous build" is the v8 and v9 runs of
2026-11-12 for the first four tasks, and the two hybrid runs of the v6 build
for the Spring flow question, which v8 and v9 did not run.

- **The module move went from 1.84 times plain shell to 0.76.** The new
  `gcal callers <file>` lists the 25 importing files, the three spellings of
  the module path and the lines that use the imported names in one answer of
  3.7 kB. One run needed 3 calls and 126k tokens (plain shell: 6 calls, 235k).
- **The LOD refactor passes 2 of 2 at 0.91 of plain shell** and 40% less wall
  time. On the previous build it was 1.31 times plain shell at best, and its
  first `gcal get` failed in every run with the backend's binding cap.
- **The one-method edit now takes 2 calls in both runs** (previous build: 3
  calls in four runs of six). Input is 3k above plain shell; that is the longer
  lane prompt.
- **Not improved: the abyssal feature** (143k against 124k on the previous
  build, two runs against three). Both runs spent a call finding the project's
  existing tests after `--tests` answered "none found". Fixed for round 2.
- **Uncached input and wall time on the smallest task are still above plain
  shell**: 184k uncached tokens against 165k over the five tasks, and 47 s
  against 36 s on the StringUtils task.

Token cost of the round: 2,096k (input 2,061k, output 34k) of the 15M budget.

## What was changed before the run

All of it is in commits `ce7d73a`, `4da6e5f`, `280a219` and `9275735`.

| Gap (from the previous document) | Cause found | Change |
|---|---|---|
| Backend query cap on large indexes | A `MATCH` with one pattern and a `WHERE` collected every candidate before filtering, in two places (the general evaluator and the single-node `LIMIT` fast path). On a graph above 100,000 nodes the lookup of the nodes of one file failed with "query exceeds intermediate binding safety cap". | The filter runs while candidates are produced; the cap counts what the query keeps. A query that keeps more than the cap still fails. |
| Small edits cost one call too many | The agent read the method, then needed a second call for the tests that exercise it. `--tests` exists but was used in one run of three. | `gcal get Symbol BigType`: when a type or file is too long to print whole and is read together with a symbol, the members of that type that call the symbol are printed with it. No flag to remember. |
| Wide refactors cost two to three times plain shell | Agents spent 6 to 8 calls finding importers; `gcal callers <module specifier>` answered "not found"; a lookup that found nothing exited 1 and cancelled the rest of an `&&` chain (20 of 117 `gcal` commands in 18 earlier runs exited non-zero, 5 of them in a chain); `callers` cut at 20 rows and was run again with `--limit`. | `gcal callers <file>` lists importers (imports, re-exports, dynamic imports, test mocks; alias, relative and extension spellings; JavaScript/TypeScript, Python, C includes, assets; it also answers for a path that no longer exists). Lookups that find nothing exit 0. `callers` and `callees` show 60 rows by default. |
| Slow first daemon start (65 s, earlier 116 s) | Not a product defect: the operations benchmark had been run while a full workspace build was using every core. Measured on a quiet machine: backend initialisation 0.3 to 1.1 s, first graph query 1.2 s more (3 s on the 195 MB LOD index), later calls at most 70 ms; daemon cold start 2.0 s on Spring and 1.3 to 1.6 s on goldeneye-tool. | The daemon issues one graph-only query right after the first answer of a session, so the graph load falls between two commands. |

The lane prompt and the skill text describe the new lookups.

## Reading of the runs

Commands of each run, from the recorded traces (`target/agent-bench/<task>/one-shot/m-gr1-a|b`).

| Task | Run a | Run b |
|---|---|---|
| StringUtils | `gcal get 'StringUtils.truncate' 'StringUtilsTests'`, patch, `gcal check; git status` | `gcal get ...truncate; gcal grep 'truncate\(' StringUtilsTests.java -C 4`, patch, check |
| abyssal | `gcal get SecurityConfig JwtAuthenticationFilter --tests` ("none found"), `rg --files src/test`, `gcal get ModelMappersTest.java`, patch, check | `gcal get` of the two classes, the same again with `--tests` plus `rg --files src/test` and `pom.xml`, patch, check |
| terax | `gcal get native.ts --tests; gcal callers native.ts`, one shell call with the move and three `gcal replace`, patch, check: 126k | `gcal callers native` ("ambiguous"), a command lost to PowerShell quoting, `gcal callers native.ts`, replaces, a repair replace (`nativeBridgeBridge`), patch, check: 232k |
| LOD | read two files and `gcal grep` (cut at 30 symbols), the grep again with `--limit 300`, two `gcal replace`, shell reads of a JSON and a Markdown file the replace had rewritten, patches, final grep, check | similar; the first grep cut, a second with `--limit 200` |
| Spring flow | three batched `gcal get` (37 kB, 33 kB, 41 kB): 133k | six batched `gcal get`, the first 84 kB: 205k |

Waste counters over the ten runs: 1 failed command (PowerShell quoting), 1
"ambiguous" answer, 6 "not found" answers inside batches, 2 cut `grep` outputs
that were followed by a rerun, 0 silent failures. `gcal check` ran in all eight
editing runs; `gcal replace` in all four refactor runs.

What the traces point to next:

1. `--tests` with no test of the symbol leaves the agent to look for the
   project's tests on its own (abyssal, both runs, one call each).
2. `gcal callers native` is "ambiguous" on terax because the index names an
   anonymous JavaScript function after the first identifier in it:
   `() => native.gitStage(..)` is indexed as a function called `native`. The
   terax index holds 13 symbols named `native`; one is real.
3. `gcal grep` fetches up to 90 matching symbols and shows 30; the rest cost a
   second call in both LOD runs.
4. `gcal replace` given directories rewrote a JSON manifest and a Markdown
   file next to the Python files (LOD run a); the agent had to read and repair
   them.

## Verification without a model

- Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` pass (101 test binaries). New tests build a graph
  of 100,001 nodes and check both forms of the filtered lookup and that an
  unfiltered query still fails.
- The two query forms that failed on the real LOD index (195 MB database) now
  answer: 133 ms warm, 3.1 s when the query is the first of the process.
- Agent: `pnpm check` passes (210 tests). New tests cover importers
  (TypeScript, Python, a moved file, a Java file answered by its type), related
  members, exit status 0 for a lookup that finds nothing, and the daemon
  warm-up order.
- `gcal callers src/modules/ai/lib/native.ts` on terax at the task's base
  commit lists 25 files, the same set as
  `grep -rlE "(modules/ai/lib/native|\.\./lib/native|[\"']\./native)[\"']" src`,
  in 0.3 s.
- Operations benchmark, unchanged in tokens: Spring 19 of 19 scenarios, 20
  calls, 11,552 output tokens (plain shell: 21 calls, 14,900); goldeneye-tool
  12 of 12, 12 calls, 2,946 tokens (plain shell: 13 calls, 4,251).

## Measured and not acted on

- A `gcal` command through the warm daemon takes 190 to 450 ms on terax
  (node start 58 ms, module load 110 ms, the rest the query). In the operations
  benchmark a `gcal` call takes 0.85 s against 0.38 s for a shell call,
  PowerShell start included. An agent turn takes 10 to 30 s, so this was left
  alone.
- The harness starts the daemon before the prompt and records the time:
  about 10 s on the StringUtils and LOD tasks, 1 to 5 s on the others. It is
  not part of the wall time above. The 10 s are not explained yet.

## Limits

- Two runs per task. The spread inside a task is large (terax: 126k and 232k;
  Spring flow: 133k and 205k), so a median of two is an indication, not an
  estimate. The plain-shell numbers are the three runs of 2026-11-11 and were
  not repeated; the model and the Codex version are the same
  (`gpt-5.6-terra`, reasoning high, codex-cli 0.159.0).
- The GCAL lane is the hybrid lane: the agent may use `gcal` and shell tools.
  The GCAL daemon is started before the prompt.
- Fairness: the GCAL lane has `gcal check`, a parse check of changed files,
  which the one-shot policy allows; the plain-shell lane has no equivalent and
  runs no build or test either.
- Seven of the twelve matrix tasks were not run in this round.
- The importers lookup was checked against a text search on one repository
  only. Rust `use`/`mod`, Go packages and Java imports are answered through
  the type the file is named after, which is untested on real repositories.
