# GCAL improvement goal, round 2: four fixes from the traces, four tasks not measured since v5

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-1.md`.

## Short answer

Seven agent runs, one per task, all seven passed their graders. Three more
tasks were refused by the harness before any model call (see "Harness
failures"); they are measured in round 3.

| Task | GCAL, this round | Plain shell (median of 3 or 4) | Input ratio | Wall ratio |
|---|---|---|---|---|
| abyssal feature | 131k input, 3 calls, 64 s | 155k, 4 calls, 82 s | 0.85 | 0.78 |
| terax module move | 195k, 5 calls, 129 s | 235k, 6 calls, 138 s | 0.83 | 0.93 |
| LOD signature refactor | 320k, 6 calls, 156 s | 495k, 10 calls, 286 s | 0.65 | 0.55 |
| terax fuzzy feature | 115k, 3 calls, 85 s | 112k, 3 calls, 98 s | 1.03 | 0.87 |
| PyFinance bug | 202k, 5 calls, 97 s | 192k, 4 calls, 94 s | 1.05 | 1.03 |
| goldeneye rename | 113k, 3 calls, 66 s | 116k, 3 calls, 69 s | 0.97 | 0.95 |
| cbm architecture document | 806k, 16 calls, 212 s | 1,615k, 19 calls, 267 s | 0.50 | 0.79 |
| **Sum** | **1,883k, 41 calls, 808 s** | **2,919k, 49 calls, 1,035 s** | **0.64** | **0.78** |

One run per task: each cell is a single observation against a median.

- The three tasks repeated from round 1 moved in the expected direction with
  the fixes that targeted them: LOD 451k to 320k, abyssal 143k to 131k. The
  module move (195k) is inside the spread of round 1 (126k, 232k).
- The four tasks last measured with GCAL v5 (the `gcal`-only lane) are at or
  below plain shell, where two of them were well above it: the terax fuzzy
  feature was 1.67 times plain shell and is 1.03; the goldeneye rename was
  1.06 and is 0.97; the cbm document was 0.63 and is 0.50; PyFinance was 0.90
  and is 1.05, the one task of the round that costs more than plain shell.
- Uncached input over the seven tasks: 242k against 282k for plain shell.

Token cost of the round: 1,910k (input 1,883k, output 27k). Cumulative: 4,006k
of the 15M budget.

## What was changed before the run

Commits `d254f7e` and `4a90ee6`.

| Seen in round 1 | Cause | Change |
|---|---|---|
| `gcal callers native` answered "ambiguous: 13 symbols" (terax) | The index named an anonymous JavaScript function after the first identifier inside it: `() => native.gitStage(..)` became a function called `native`. | A function value takes the name of the variable, class field, object property or assignment target it is the value of, and that declaration is not a second symbol. A function passed as an argument is not a symbol; its calls belong to the function around it. The terax index went from 9,389 to 6,604 nodes; `native` is one symbol. |
| Both abyssal runs spent a call finding existing tests after `--tests` said "none found" | Nothing in the answer said where tests live or what they look like. | With no test of the symbol, `--tests` prints the nearest test files and the shortest of them whole (up to 80 lines). |
| Both LOD runs repeated a `grep` that was cut at 30 symbols | `grep` fetched 90 matching symbols and showed 30. | Without `--limit`, every match is shown when the backend returned them all. |
| A `replace` over directories rewrote a JSON manifest and a Markdown file (LOD) | No way to restrict by file kind. | `gcal replace ... -g '*.py'`; files left out that still match are named. |
| `callers` read at most 80 text references without saying so | Silent cap. | The answer says how many places matched and how to list them. |

## Reading of the runs

| Task | Commands (from the trace) | What it shows |
|---|---|---|
| abyssal | `gcal get SecurityConfig JwtAuthenticationFilter --tests` (the example test is printed), `gcal get AuthenticationServiceTest.java pom.xml`, patch, check | The printed example did not stop the agent from reading a nearer test and the build file. One call saved against round 1 run a, none against run b. |
| terax module move | `Get-Content TERAX.md; gcal callers native.ts`, a read of the module, a read of two test files, one shell call with the move and six `gcal replace`, check | No "ambiguous" answer. |
| LOD | `gcal get canonical pairs strict_json --tests; gcal callers canonical` (the first call that failed with the binding cap before round 1), two greps, a range read, patch, `gcal replace ... -g '*.py'` three times, check | No cut-and-repeat; the JSON and Markdown files were left alone. |
| terax fuzzy | `gcal get fuzzyScore fuzzyBest --tests`, `gcal get fuzzy.ts fuzzy.test.ts`, patch, check | The second call reads both files whole after the first printed the two functions and their tests. |
| PyFinance | `gcal grep`, `gcal get normalization.py --tests` (prints the file and nothing about tests), `gcal get normalize_price_frame --tests`, `gcal get tests/test_data.py`, patch, check | `--tests` on a file does nothing, and the tests it prints for a symbol come without the imports and fixtures a new test needs: the agent read the whole test file. |
| goldeneye rename | read, `gcal replace`, check | Nothing to remove. |
| cbm document | 16 commands, 6 of them file listings in shell (`rg --files | rg -i ...`, `Get-ChildItem`), `gcal search index` (537 bytes), four large batched `gcal get` | Orientation by file name goes through shell, one listing at a time. |

Waste counters over the seven runs: 1 failed command (an `rg` pipeline that
matched nothing), 3 cut outputs, 0 "ambiguous", 0 "not found", 0 silent
failures.

## Harness failures

Three tasks did not run. No model was called for them.

- `vivwallet-feature-dirty` and `agentsview-sync-flow-qa-dirty`: the overlay
  patch that creates the dirty starting state did not apply. The merge of
  2026-11-13 checked the patch files out again, `core.autocrlf` gave them CRLF
  line endings, and the two repositories keep LF. Fixed in `b50ef80` and
  `0ae095b`: the patches are stored with LF (`.gitattributes`) and the harness
  tries a patch as stored, then with LF, then with CRLF.
- `spring-sensitive-value-redaction-level0`: its configuration named a `gcal`
  path of another machine layout. Fixed in `b50ef80` for the three redaction
  configurations.

## Verification without a model

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (101 test binaries) and the 159-language corpus
  (`cargo test -p goldeneye-index --features full-grammar-tests`) pass. A new
  corpus test indexes a TypeScript file with the five shapes of function value
  and checks the names, the labels and the absence of stolen names.
- `pnpm check` passes (212 tests); the harness tests pass (88).
- Operations benchmark unchanged: Spring 19 of 19, 11,552 tokens;
  goldeneye-tool 12 of 12, 2,946 tokens.
- On terax at the task's base commit, re-indexed with the new binary:
  `gcal callers native` lists 65 uses; before, it listed 13 candidates.

## Limits

- One run per task in this round.
- The index change was checked on one TypeScript repository and the language
  corpus. Call edges that used to start at the wrongly named functions now
  start at the enclosing function; the effect on `callers` precision was not
  measured.
- The 5 to 12 s the harness records for starting the daemon on the large
  indexes (before the prompt, not part of wall time) is still unexplained.
  Outside the harness, on a fresh copy of the 195 MB LOD database, the backend
  alone answers in 0.3 s and `gcal status` through a cold daemon in 0.8 s.
