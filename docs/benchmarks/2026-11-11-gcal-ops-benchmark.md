# GCAL vs plain shell: operation-level benchmark and optimization

Date: 2026-11-11

> Naming: when this was measured the agent CLI was a separate repository named
> ACK and was invoked as `ack`. It is now GCAL, in `agent/` of this repository,
> and this document uses the current names. Run directories on the machine that
> produced the numbers keep the old names (`m-goldeneye-ack-*`, engine
> `goldeneye-ack`).

## Question

GCAL over Goldeneye often failed to be faster or more token-efficient than not
using it. Where does it lose, on which kinds of tasks, and what fixes that?

## Method

Two measurements, cheapest first.

1. **Recorded agent runs.** The Codex event logs already under
   `target/agent-bench/` (Spring redaction Level 0, 3 GCAL runs and 3 vanilla
   runs) were mined per command: subcommand, exit code, output bytes.
2. **Operation-level benchmark** (`tools/agent-bench/bin/benchmark-ops.mjs`,
   new). No model in the loop. Each scenario is one information need with the
   evidence that proves it was met and one scripted command chain per lane. A
   lane stops at the first step whose cumulative output holds all evidence.
   29 scenarios over two corpora: Spring `core`/`beans`/`context` (Java, 2,614
   files) and this repository (Rust and TypeScript, 722 files).

Lanes: `gcal` (GCAL CLI over the Goldeneye daemon) and `vanilla` (`rg`,
`Get-Content`, `git ls-files`). Both run through `pwsh -NoProfile -Command`,
as Codex runs them. GCAL chains start from the name an agent naturally types
(observed in the recorded runs); vanilla chains are deliberately tight, so the
comparison is conservative for GCAL.

Cost is reported in uncached-token equivalents:
`calls x 25,000 x 0.1 + output tokens x 2`. Every tool call adds a model turn
that re-reads the cached context; every output token is read once uncached and
re-read cached on about ten later turns. Tokens are estimated as characters / 4.
The constants are a model, not a measurement; calls, tokens, and wall time are
measured.

## What the recorded agent runs showed

Spring redaction Level 0, median of 3 runs per lane:

| Metric | Vanilla | GCAL (before) |
|---|---:|---:|
| Commands issued | 13 | 33 |
| Tool output | 201–245 KB (2 runs inspected) | 49–62 KB |
| Input tokens | 1,149,062 | 1,232,875 |
| Wall time | 356 s | 488 s |

GCAL returned about a quarter of the bytes and still lost, because it needed
2.5x the calls. Under the cost model one extra call costs as much as about 5 KB of
output. The extra calls had concrete causes, all visible in the logs:

| Cause | Example from the logs |
|---|---|
| Natural names rejected | `gcal get org.springframework.validation.DataBinder.getBindingResult` → `symbol was not found`; the backend name doubles the file segment (`…DataBinder.DataBinder.getBindingResult`) |
| Every read took two calls | `search` to learn the identifier, then `get` |
| Large types failed | `gcal get …DataBinder` → `snippet exceeds bounds: 54267 bytes/1509 lines` |
| Empty answers | text-like queries (`"typeMismatch"`, `implements X`) printed nothing; 3–5 empty calls per run |
| Guessed flags | `--path` → `error: unknown option` |
| Identifier bloat | every row carried a ~150-character project- and path-prefixed name |
| Warm-up habits | `gcal status` and `gcal --help` in every run |

## Operation-level results

`gcal before` is GCAL at `09394ce` with the Goldeneye binary built on 2026-07-27.
`gcal after` is the working tree of both repositories.

### Spring (17 scenarios)

| Lane | Scenarios met | Calls | Failed calls | Output tokens | Wall time | Cost |
|---|---:|---:|---:|---:|---:|---:|
| gcal before | 16/17 | 44 | 7 | 16,776 | 26.3 s | 143,552 |
| gcal after | 17/17 | 18 | 0 | 6,787 | 13.9 s | 58,574 |
| vanilla | 17/17 | 19 | 0 | 10,300 | 7.7 s | 68,100 |

### goldeneye-tool (12 scenarios)

| Lane | Scenarios met | Calls | Failed calls | Output tokens | Wall time | Cost |
|---|---:|---:|---:|---:|---:|---:|
| gcal before | 11/12 | 25 | 6 | 5,872 | 13.5 s | 74,244 |
| gcal after | 12/12 | 12 | 0 | 2,773 | 7.2 s | 35,546 |
| vanilla | 12/12 | 13 | 0 | 4,259 | 5.0 s | 41,018 |

Against its former self GCAL costs 59% and 52% less. Against tight shell
commands it now costs 14% and 13% less and returns 34–35% fewer tokens, but
still takes 1.4–1.8x the wall time.

### By kind of task (cost: gcal before / gcal after / vanilla)

| Category | Spring | goldeneye-tool | Verdict after |
|---|---|---|---|
| Read known symbols | 34,078 / 13,180 / 16,444 | 28,292 / 9,010 / 10,342 | GCAL wins |
| Read several symbols at once | 11,336 / 3,836 / 7,778 | 10,592 / 3,480 / 4,750 | GCAL wins clearly |
| Outline of a type or file | 24,204 / 8,292 / 11,768 | 11,550 / 6,278 / 6,450 | GCAL wins on Java, ties on Rust |
| Callers, callees, call chains | 43,258 / 15,258 / 14,442 | 5,306 / 5,302 / 8,206 | tie on Java, GCAL wins on Rust |
| Subtypes and implementors | 10,856 / 5,386 / 5,272 | 5,088 / 2,620 / 2,586 | tie |
| Literals and constants | 10,908 / 5,996 / 5,826 | 5,386 / 2,940 / 2,884 | tie |
| Locate a file by type name | 3,054 / 2,896 / 2,582 | 2,966 / 2,818 / 2,578 | shell slightly ahead |
| Project layout | 5,858 (failed) / 3,730 / 3,988 | 5,064 (failed) / 3,098 / 3,222 | tie |

GCAL pays off where it can return exactly the wanted body or a structural
answer in one call. For plain text lookups and file location it can only
match `rg`, never beat it; there the result is a tie on cost and a loss on
latency.

## Changes

### GCAL (`goldeneye-code-agent-layer`)

- **Any natural name resolves.** `Type`, `Type.member`, `pkg.Type.member`,
  `Type::member`, `Type#member`, `File.ext`, `File.ext:42`, `File.ext:10-60`.
  Wrong leading segments are dropped; overloads come back together.
- **Short names out.** Rows show `Type.member  Kind  path:line  declaration`
  instead of the backend identifier, and every source block is headed by its
  `path:start-end`, so it is directly usable as an edit target.
- **`get` never dead-ends.** A type or file comes back whole up to 500 lines
  and as a member outline above that (`--full` overrides); a type whose
  methods live outside its body (Rust `impl`, Go) lists them; an ambiguous name
  prints up to three small candidates or a pick list.
- **Reads survive edits.** Source is read from the working tree. When a file
  changed after indexing, the declaration is re-located by text and labeled as
  such. Before, `get` failed with `indexed source file is stale` until a
  re-index succeeded.
- **`search` answers in one call.** Several queries per call, overloads
  collapsed into one row, declaration text per row, and a text fallback when
  no symbol matches.
- **`callers` adds text references** to call-graph edges, with the call-site
  line. For `addError` in Spring the graph alone returned 2 of 8 call sites,
  and none of the calls made through the `BindingResult` interface.
- **New:** `grep` (rg-style regex with a literal fallback, grouped by file and
  enclosing symbol; `[paths...]` and `-C <n>` print matching lines with
  context), `impls`
  (subtypes), `outline`, and a compact directory-based `arch`.
- Per-command backend round trip for the project root removed (about 60 ms).
- Workflow skill, `AGENTS.md` rules, and the benchmark lane prompt rewritten
  around "one call per question; name the target; batch".

### Goldeneye

- `search_code` scans files on up to 8 threads, queries graph nodes only for
  files that contain a match, and takes node degrees from the engine's cached
  graph instead of loading every edge per call. Spring corpus, warm, four
  selective patterns: 1.6–1.8 s → 50–70 ms. A pattern matching most files
  (`return`): 1.7 s → 0.9 s. Results are identical apart from `elapsed_ms`.
- `search_code` skips a file deleted after indexing instead of failing the
  whole search.

## Verification

- GCAL: `pnpm test` 164 passed (22 new), `pnpm lint` and `pnpm build` clean.
- `tools/agent-bench`: `node --test` 81 passed (8 new).
- Operation-level benchmark after round 2 fixes (label `gcal-v3`, two scenarios
  added to Spring: a 400-line type read whole, usages with context): Spring GCAL
  19/19, 20 calls, 0 failed, 11,536 tokens against vanilla 21 calls, 14,900
  tokens; goldeneye-tool GCAL 12/12, 12 calls, 2,914 tokens against vanilla 13 calls, 4,259.
- Goldeneye: `cargo test --workspace` 495 passed, 0 failed;
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  and `cargo xtask architecture verify` clean (with `GOLDENEYE_GRAMMAR_PACK_DIR` set).

## Agent-level check

One paired one-shot run per task with the new GCAL (`gpt-5.6-terra`, reasoning
high, Codex CLI 0.159.0, no agent-side build or test, held-out grader). `n = 1`
per lane, so these are single observations, not estimates.

| Task | Lane | Grader | Input | Cached | Uncached | Output | Reasoning | Tool calls | Tool output | Wall |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Spring redaction Level 0 | GCAL | PASS | 907,651 | 833,536 | 74,115 | 11,524 | 6,372 | 15 | 135 KB | 392 s |
| | vanilla | PASS | 482,643 | 416,000 | 66,643 | 8,841 | 3,935 | 9 | 138 KB | 238 s |
| StringUtils unicode truncate | GCAL | PASS | 110,765 | 83,968 | 26,797 | 1,511 | 625 | 3 | 8.8 KB | 50 s |
| | vanilla | PASS | 82,807 | 59,648 | 23,159 | 1,244 | 460 | 2 | 3.4 KB | 39 s |

**The operation-level gain did not carry over.** GCAL still used 88% and 34%
more input tokens and 65% and 28% more wall time than plain shell. Both lanes
passed, no GCAL call failed or came back empty, and no protocol violation was
recorded, so the old failure modes are gone; what remains is turn count.

What the traces show:

- **GCAL made more turns, not smaller outputs.** Level 0: 15 calls against 9
  with the same total output (135 KB vs 138 KB). The agent batched as told
  (13 shell calls carrying 26 `get` and 21 `search`) but explored further.
- **Outline-then-read costs a turn that whole-file reads do not.** Vanilla
  read `AbstractBindingResult.java` whole in one call; GCAL returned an outline
  for types over 150 lines, and the agent came back for members and line
  ranges (`MergedAnnotations.java:280-390`, then `:390-450`).
- **No match-with-context.** On the small task vanilla got everything from one
  `rg -C 8 truncate` over two files. GCAL returned the method plus an outline of
  the test file, then needed a second call for the ranges.
- **Both tasks name their classes.** Neither needs discovery across an unknown
  codebase, which is where a graph should pay for itself; the task mix does not
  yet test that.

No one-shot run exists for the old GCAL, so its agent-level cost in this mode
is unknown. The earlier scored runs included agent-side Gradle verification
and are not comparable with these.

The harness needed one fix to run at all: it passed
`features.code_mode_host=false`, which on Codex 0.159 leaves the agent with no
way to execute commands (`Code Mode is unavailable because code-mode host is
disabled`). The flag is now opt-in through `codex_code_mode_host: false`.

### Round 2: whole types and match-with-context

Two changes aimed at the extra turns above, then the same two pairs again:
`get` returns types and files whole up to 500 lines (was an outline above 150),
and `grep <pattern> [paths...] -C <n>` prints matching lines with context.

| Task | Lane | Grader | Input | Cached | Uncached | Output | Reasoning | Tool calls | Tool output | Wall |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Spring redaction Level 0 | GCAL | PASS | 1,500,394 | 1,401,344 | 99,050 | 11,803 | 6,246 | 17 | 250 KB | 374 s |
| | vanilla | PASS | 858,283 | 775,680 | 82,603 | 10,523 | 4,776 | 10 | 212 KB | 282 s |
| StringUtils unicode truncate | GCAL | PASS | 138,402 | 109,312 | 29,090 | 1,569 | 617 | 4 | 14.3 KB | 54 s |
| | vanilla | PASS | 85,383 | 60,672 | 24,711 | 1,503 | 758 | 2 | 7.4 KB | 44 s |

**The gap did not close.** GCAL input was 1.75x vanilla on Level 0 (round 1:
1.88x) and 1.62x on the small task (round 1: 1.34x). GCAL lost all four pairs
across both rounds.

**Run-to-run noise is as large as the effect being measured.** The vanilla
lane did not change between rounds, yet its Level 0 input went from 482,643 to
858,283 (+78%). With one run per lane, neither round can show whether the two
changes helped or hurt.

What the round 2 traces show, each a defect of the round 2 build:

- **`grep` read patterns as literal text.** Agents write rg-style regex. On the
  small task `gcal grep "truncate\("` returned `# no text matches` and cost the
  extra call; on Level 0 `'rejectValue|typeMismatch|initDirectFieldAccess'`
  found nothing and was redone as three greps.
- **`--full` was advertised in the lane prompt and used on everything.** Four
  consecutive `gcal get <five large types> --full` calls each hit the 48 KB cap,
  with the largest types cut mid-body (`DataBinder`, 1,380 lines). GCAL tool
  output rose to 250 KB.
- **The harness flagged `gcal grep File.java` as a raw `grep`.** Both GCAL runs
  were recorded as failed with two `direct_source_read` violations each; the
  grader passed both.
- Three GCAL calls exited non-zero on Level 0: members inherited from a
  supertype (`BeanWrapperImpl.getPropertyTypeDescriptor`) are not found under
  the subtype, a bare `Scope.java` is ambiguous, and files the agent had just
  created are not indexed and so not found by name.

Fixed after round 2, **not yet measured with an agent**:

- `grep` takes a regex as rg does; a pattern that is not a valid regex, or that
  matches nothing as one, is retried as literal text (`-F` forces literal).
  The text fallback of `search` does the same.
- `--full` is no longer in the lane prompt or the skill; a batch over the
  output budget turns its largest types into outlines instead of cutting them,
  with or without `--full`. The same five-type call now returns 28 KB: four
  types whole and an outline of `DataBinder`.
- The violation detector unwraps `pwsh -Command "..."`, splits on separators
  outside quotes only, and skips `gcal` invocations. Re-checked on the stored
  round 2 logs: 0 GCAL commands flagged, vanilla reads still detected.
- The "N more matching symbols" note counted lines, not symbols.

Inherited-member lookup and reading files created after indexing are not fixed.

Codex tokens spent on round 2: 2.61M (estimate given beforehand: 1.6M; the
Level 0 pair alone used 2.38M).

### Round 3: three pairs on the small task, after the round 2 fixes

StringUtils unicode truncate only, lanes interleaved, all six graders PASS.

| Lane | Run | Input | Cached | Uncached | Output | Tool calls | Wall | Valid |
|---|---|---:|---:|---:|---:|---:|---:|---|
| GCAL | 1 | 110,590 | 83,968 | 26,622 | 1,223 | 3 | 40 s | yes |
| GCAL | 2 | 111,067 | 84,992 | 26,075 | 1,240 | 3 | 42 s | yes |
| GCAL | 3 | 87,734 | 62,720 | 25,014 | 1,322 | 2 | 41 s | no: the agent ran `rg` instead of `gcal` |
| vanilla | 1 | 86,757 | 62,720 | 24,037 | 1,458 | 2 | 42 s | yes |
| vanilla | 2 | 83,573 | 59,648 | 23,925 | 1,387 | 2 | 41 s | yes |
| vanilla | 3 | 86,610 | 62,720 | 23,890 | 1,278 | 2 | 38 s | yes |

- **On this task the result is stable: GCAL costs 28% more input and the same
  wall time.** Vanilla input spread over three runs is 4%, the two valid GCAL
  runs differ by 0.4%. (The 78% swing seen on Level 0 is a property of that
  long task, not of the harness.)
- **The whole difference is one tool call.** GCAL 3 calls, vanilla 2; one call
  on this task is about 24,000 cached input tokens. Uncached input differs by
  about 2,300 tokens, output is equal.
- **Plain shell needs one discovery call here** (`rg -C` over the two named
  files), so GCAL can tie at best. A task has to need several shell calls
  before a graph tool can come out ahead; this one cannot show that.
- GCAL run 3 broke the lane rule and used `rg`; it is a vanilla run in effect
  and matches the vanilla numbers.

Why GCAL took the second call:

- Run 1: `gcal get <802-line test file>` returned an outline, then a range read.
  The agent did not use `grep -C`.
- Run 2: `gcal grep "truncate\(" ... -C 3` worked (regex fix), but
  `gcal get StringUtils.truncate` printed the method bodies without their
  Javadoc, so the agent read `StringUtils.java:1388-1423` to get it.

Fixed after round 3, not measured with an agent: `get <symbol>` now starts at
the doc comment and annotations directly above the declaration
(`StringUtils.truncate` prints `1388-1399` instead of `1397-1399`).

Codex tokens spent on round 3: 0.57M.

## Not verified

- The agent-level result is one run per lane per round. Run-to-run spread is
  large (vanilla Level 0 input: 482,643 and 858,283 in two identical one-shot
  runs; 1.02M to 1.83M in earlier scored reports), so the size of the gap is
  uncertain; its direction matches every comparison so far.
- The fixes made after round 2 (regex `grep`, `--full` handling) are covered by
  unit tests, the operation-level benchmark, and replays of the round 2
  commands, not by an agent run.
- The cost model constants are assumptions. Conclusions that depend on them
  (which lane "wins" a near tie) are soft; call and token counts are not.
- Text re-location in edited files is heuristic. It is covered for Java, Rust,
  and Python declaration shapes only.

## Open problems found

| Problem | Effect | Where |
|---|---|---|
| Launcher overhead on Windows | an GCAL call takes about 620 ms through `pwsh` and the script shim, about 300 ms when `node` is spawned directly; `rg` takes about 380 ms | GCAL packaging |
| Re-index is all-or-nothing | `index_repository` returns `rejected_syntax` and keeps the old graph when any already-indexed file has a parse diagnostic; the response gives a count but not the file | `goldeneye-index/src/service/indexing.rs` |
| Re-index reparses every file | 3 s for 722 files with 6 changed | same |
| Calls through interface-typed receivers are not linked | `callers` of an interface method is empty in the graph | `goldeneye-tree-sitter-index` |
| Index noise | Java package declarations indexed as a `Class` named `org`; Rust closures indexed as `Function`s named after their parameters | `goldeneye-tree-sitter-index` |
| First call after daemon start | about 1 s to load the project graph | `goldeneye-query` cache |

## Reproduce

```powershell
# once per corpus: register it in an isolated state directory
$env:GCAL_HOME = "target/ops-bench/spring/gcal-home"
$env:GOLDENEYE_DB_PATH = "target/ops-bench/spring/goldeneye.db"
gcal init   # from the corpus root

node tools/agent-bench/bin/benchmark-ops.mjs `
  --scenarios tools/agent-bench/ops/spring.json `
  --repo D:\Dev\IdeaProjects\spring-framework `
  --state target/ops-bench/spring `
  --gcal-entry agent\dist\main.js `
  --goldeneye target/release/goldeneye.exe `
  --rg-dir <dir containing rg.exe> --label optimized
```

Reports: `target/ops-bench/<corpus>/report-<label>.{json,md}`.
