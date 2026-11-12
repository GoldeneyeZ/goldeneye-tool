# ACK vs plain shell on 12 agent tasks: three repetitions

Dates: 2026-11-11 (repetition 1) and 2026-11-12 (repetitions 2 and 3).
Follows `2026-11-11-ack-ops-benchmark.md`. Continued in
`2026-11-12-ack-daemon-hybrid-lanes.md`, which measures ACK v6, the ACK daemon
and a lane where the agent may use both ACK and shell tools.

## Question

On which kinds of task does an agent that uses ACK (the agent CLI over
Goldeneye) spend fewer tokens than the same agent with plain shell tools, and
does it still get the task right?

## Short answer

With the ACK build used in repetitions 2 and 3 (two runs per task) against
plain shell (three runs per task):

- ACK passed 22 of 24 graded runs; plain shell passed 36 of 36. The older ACK
  build of repetition 1 passed 9 of 12.
- Summed over the 12 tasks, ACK used 19% less input (10.42M against 12.94M
  tokens for the same two repetitions). Almost all of that is cached input:
  uncached input is 4% lower (1.17M against 1.21M) and output is equal (141k
  against 146k).
- The saving comes from three tasks that are about understanding code. ACK was
  cheaper on them in all 9 paired runs, by 15% to 64%.
- ACK was dearer in every run on the one-method edit (one extra call), on the
  abyssal feature and on the 26-file module move (1.7 to 2.7 times).
- On the other five tasks the sign of the difference changed between
  repetitions; these tables cannot rank the lanes there.
- ACK took more wall time in every repetition: 11% and 15% more with the
  current build, 22% more with the old one.

## Method

- Harness: `tools/agent-bench/bin/benchmark-agent-tasks.mjs --one-shot`, Codex
  `exec`, model `gpt-5.6-terra`, reasoning high. One run per lane per task per
  repetition.
- Lanes: `goldeneye-ack` (warm index; source reads and searches must go through
  `ack`) and `vanilla` (`rg`, `Get-Content`, no index).
- Every task has a held-out grader that fails on the start state and passes on
  a reference solution. Agents are told not to run tests.
- Dirty tasks apply an uncommitted patch after indexing (`overlay.patch_file`),
  so the index is older than the working tree, as in normal use.
- Tokens are the provider's counts: input (cached and uncached) and output.
- ACK builds: repetition 1 used build v4
  (`target/agent-bench/authoring/ack-dist-v4`). Repetitions 2 and 3 used build
  v5 (`ack-dist-v5`), which holds the fixes listed under "ACK, after
  repetition 1". The vanilla lane, the harness, the prompts and the Goldeneye
  binary did not change between repetitions 2 and 3.

| Task | Repository | Kind | Tree | Grader |
|---|---|---|---|---|
| spring-stringutils-unicode-truncate | spring-framework (Java, large; 3 modules indexed) | one-method edit | clean | held-out script |
| terax-fuzzy-diacritics | terax-ai (TypeScript) | small feature | clean | held-out script |
| abyssal-public-endpoints | abyssal-zenith (Java, clean clone) | small feature | clean | held-out script |
| pyfinance-behavior-bug | PyFinance (Python, small) | bug from a behaviour report | clean | held-out `unittest` |
| vivwallet-feature-dirty | ViV-Wallet (Java) | endpoint feature | dirty | structural + syntax |
| goldeneye-rename-refactor-dirty | goldeneye-tool (Rust, clean clone) | small rename | dirty | structural |
| terax-module-refactor | terax-ai | move and rename a module, 26 files | clean | structural + `tsc` + tests |
| lod-signature-refactor | LOD (Python, 182 MB of JSON datasets) | move and rename a function, 75 call sites in 40 files | clean | structural + parse |
| spring-sensitive-value-redaction-level0 | spring-framework | feature needing exploration | clean | held-out script |
| spring-event-listener-flow-qa | spring-framework | question about a flow | clean | fact checklist (14 facts, 11 required) |
| agentsview-sync-flow-qa-dirty | agentsview (Go, 1,689 indexed files) | question about a flow | dirty | fact checklist |
| cbm-pipeline-architecture-doc | codebase-memory-mcp (C, 680 indexed files) | write an architecture document | clean | fact checklist on the document |

## Result

### Current ACK build against plain shell

Each cell is ACK / vanilla. ACK: repetitions 2 and 3 (build v5), so its
"median" is the mean of two runs. Vanilla: repetitions 1 to 3. Input shows the
median and the range. `*` marks a verdict regraded after a grader fix (see
"Grader and harness defects").

| Task | Grader pass | Input tokens | ACK/vanilla input | Uncached input | Output | Tool calls | Wall |
| --- | --- | --- | --- | --- | --- | --- | --- |
| spring-stringutils-unicode-truncate | 2/2 / 3/3 | 109k (108k–110k) / 86k (86k–88k) | 1.26 | 26k / 14k | 1k / 1k | 3 / 2 | 47 s / 36 s |
| terax-fuzzy-diacritics | 2/2 / 3/3 | 187k (163k–210k) / 112k (111k–244k) | 1.67 | 17k / 27k | 5k / 4k | 5 / 3 | 132 s / 98 s |
| abyssal-public-endpoints | 2/2 / 3/3 | 185k (180k–190k) / 155k (151k–155k) | 1.20 | 34k / 28k | 3k / 3k | 5 / 4 | 96 s / 82 s |
| pyfinance-behavior-bug | 2/2 / 3/3 | 180k (159k–201k) / 201k (183k–234k) | 0.90 | 36k / 36k | 3k / 3k | 5 / 4 | 87 s / 100 s |
| vivwallet-feature-dirty | 2/2 / 3/3 | 239k (220k–258k) / 270k (212k–271k) | 0.88 | 29k / 43k | 5k / 6k | 6 / 6 | 148 s / 153 s |
| goldeneye-rename-refactor-dirty | 2/2 / 3/3 | 124k (111k–136k) / 116k (111k–136k) | 1.06 | 11k / 18k | 2k / 2k | 4 / 3 | 69 s / 69 s |
| terax-module-refactor | 2/2 / 3/3* | 588k (574k–602k) / 235k (216k–350k) | 2.50 | 55k / 29k | 9k / 5k | 12 / 6 | 270 s / 138 s |
| lod-signature-refactor | 1/2 / 3/3 | 615k (520k–711k) / 495k (254k–849k) | 1.24 | 62k / 42k | 12k / 11k | 11 / 10 | 353 s / 286 s |
| spring-sensitive-value-redaction-level0 | 1/2 / 3/3 | 814k (658k–971k) / 809k (747k–1482k) | 1.01 | 72k / 81k | 12k / 10k | 12 / 13 | 352 s / 277 s |
| spring-event-listener-flow-qa | 2/2 / 3/3 | 174k (134k–215k) / 375k (373k–415k) | 0.47 | 41k / 52k | 4k / 6k | 4 / 9 | 128 s / 172 s |
| agentsview-sync-flow-qa-dirty | 2/2 / 3/3 | 972k (941k–1003k) / 1221k (1136k–1542k) | 0.80 | 115k / 108k | 6k / 7k | 15 / 15 | 336 s / 201 s |
| cbm-pipeline-architecture-doc | 2/2 / 3/3 | 1025k (790k–1259k) / 1615k (1528k–1775k) | 0.63 | 85k / 103k | 8k / 9k | 19 / 19 | 271 s / 267 s |

### Each repetition

ACK input / vanilla input = ratio, one run each.

| Task | Rep 1 (ACK v4) | Rep 2 (ACK v5) | Rep 3 (ACK v5) |
|---|---|---|---|
| spring-stringutils-unicode-truncate | 134k / 86k = 1.55 | 108k / 88k = 1.22 | 110k / 86k = 1.28 |
| terax-fuzzy-diacritics | 140k / 111k = 1.26 | 163k / 244k = 0.67 | 210k / 112k = 1.88 |
| abyssal-public-endpoints | 155k / 155k = 1.00 | 190k / 151k = 1.26 | 180k / 155k = 1.17 |
| pyfinance-behavior-bug | 219k / 201k = 1.09 | 159k / 183k = 0.87 | 201k / 234k = 0.86 |
| vivwallet-feature-dirty | 207k / 270k = 0.77 | 220k / 212k = 1.04 | 258k / 271k = 0.95 |
| goldeneye-rename-refactor-dirty | 189k / 116k = 1.63, ACK failed | 111k / 136k = 0.81 | 136k / 111k = 1.23 |
| terax-module-refactor | 503k / 235k = 2.14, ACK failed | 574k / 216k = 2.66 | 602k / 350k = 1.72 |
| lod-signature-refactor | 379k / 254k = 1.49, ACK failed | 711k / 849k = 0.84 | 520k / 495k = 1.05, ACK failed |
| spring-sensitive-value-redaction-level0 | 820k / 809k = 1.01 | 658k / 1482k = 0.44 | 971k / 747k = 1.30, ACK failed |
| spring-event-listener-flow-qa | 169k / 415k = 0.41 | 215k / 375k = 0.57 | 134k / 373k = 0.36 |
| agentsview-sync-flow-qa-dirty | 757k / 1136k = 0.67 | 941k / 1221k = 0.77 | 1003k / 1542k = 0.65 |
| cbm-pipeline-architecture-doc | 1366k / 1615k = 0.85 | 1259k / 1528k = 0.82 | 790k / 1775k = 0.45 |

Sums over the 12 tasks:

| Repetition | Graders passed (ACK / vanilla) | Input | Uncached input | Output | Tool calls | Wall |
|---|---|---|---|---|---|---|
| 1 (ACK v4) | 9 / 12 | 5.04M / 5.40M | 598k / 560k | 67k / 62k | 92 / 124 | 2,162 s / 1,765 s |
| 2 (ACK v5) | 12 / 12 | 5.31M / 6.69M | 603k / 626k | 68k / 75k | 103 / 102 | 2,319 s / 2,083 s |
| 3 (ACK v5) | 10 / 12 | 5.12M / 6.25M | 562k / 586k | 73k / 71k | 92 / 95 | 2,265 s / 1,971 s |

The 72 runs spent 34.2M tokens (10.57M, 12.14M and 11.51M per repetition).

### How much a ratio means

The same lane on the same task varies a lot between runs: vanilla took 254k to
849k input on the LOD refactor, 747k to 1,482k on Spring Level 0 and 111k to
244k on the small terax feature. A ratio is a finding only where it keeps its
sign in every repetition.

- **Same sign in all three repetitions, ACK cheaper:** the Spring flow question
  (0.36 to 0.57), the agentsview flow question (0.65 to 0.77) and the cbm
  architecture document (0.45 to 0.85).
- **Same sign in both repetitions of the current build, ACK cheaper:** the
  PyFinance bug (0.87, 0.86; the old build was at 1.09).
- **Same sign in every repetition, ACK dearer:** the StringUtils one-method
  edit (1.22 to 1.55; three calls against two) and the terax module move (1.72
  to 2.66; 12 calls against 6). The abyssal feature is dearer in both
  repetitions of the current build (1.26, 1.17).
- **Sign changes:** the small terax feature, the ViV-Wallet feature, the
  goldeneye rename, the LOD refactor and Spring Level 0.

### Reading it by kind of task

- **Understanding code (two questions, one architecture document).** ACK is
  cheaper in 9 of 9 paired runs. These tasks are many reads across files, and
  ACK answers several of them per call.
- **Bug from a behaviour report.** ACK is 13% and 14% cheaper in the two paired
  runs of the current build.
- **Features.** No stable difference, except the small abyssal feature where
  ACK costs one more call.
- **One-method edit.** ACK is 22% to 28% dearer. Vanilla needs two calls; ACK
  needs three, and one call costs about 25k cached input tokens.
- **Renames and moves.** The current build passed 5 of 6 runs (the old build 0
  of 3). The 26-file move stays 1.7 to 2.7 times dearer: the ACK lane patches
  every file by hand while vanilla scripts one replacement. The next ACK build
  adds a command for this (see "ACK, after repetition 2"); the follow-up
  document measures it, and the move stays 2 to 3 times dearer.
- **Wall time.** ACK is slower in total in every repetition and on 8 of 12
  tasks, including the agentsview question where it uses fewer tokens (336 s
  against 201 s). The benchmark backend starts Goldeneye for each `ack` call
  (about 2.4 s per call on agentsview); the daemon used outside the benchmark
  does not.

## Failed ACK runs

### Repetition 1 (ACK v4)

| Task | What the grader found | Cause |
|---|---|---|
| goldeneye-rename-refactor-dirty | old name left in a test file added by the overlay and in a Markdown file | `ack grep` only searched indexed files. The test file was created after indexing, and this task's index covers `crates;xtask`, not `docs/`. The agent reported that no reference remained. **ACK defect, fixed in v5**; the `docs/` part is also a benchmark configuration choice. |
| lod-signature-refactor | `audit_curriculum.py` no longer parses (one line lost four spaces of indentation); 74 of 75 call sites done | The agent wrote 41 file patches from `ack grep -C 0` output without reading the files. `ack grep "\bcanonical\b" .` returned nothing (`.` matched no path) and cost a call. The indentation slip is the agent's; the `.` path is an **ACK defect, fixed in v5**. |
| terax-module-refactor | one new type error: `nativeMock` undefined at `edit.test.ts:204` | The agent's own edit. Three ACK calls were wasted before it: `ack get ... -C 0` (unknown option), `ack get transport.ts` (two files share the name, reported as not found), and a pattern with nested quotes. The first two are **ACK defects, fixed in v5**. |

### Repetition 3 (ACK v5)

| Task | What the grader found | Cause |
|---|---|---|
| lod-signature-refactor | `src/lod/training/pilot.py` does not parse (line 240 lost four spaces of indentation); 73 of 75 call sites in 39 of 40 files | The same failure as in repetition 1: the agent hand-patched every call site from `ack grep` output over 8 `ack` calls and 3 patches. No ACK output was wrong. The lane has no bulk edit, which is what `ack replace` (v6) is for. |
| spring-sensitive-value-redaction-level0 | `spring-context:compileTestJava` fails at `DataBinderTests.java:1877` | A test the agent added passes a lambda to `setValidator`; `Validator` has two abstract methods. The held-out tests share that module, so they could not be compiled. The production change was not graded. The agent's own error; agents may not build in this benchmark. |

Repetition 2 had no failure in either lane.

## Defects found and fixed

### ACK, after repetition 1 (build v5, measured by repetitions 2 and 3)

| Seen in | Before | After |
|---|---|---|
| goldeneye rename | `ack grep` did not search files created after indexing or outside the index | such files are scanned in the working tree: new files always, files outside the index when paths are given |
| LOD | `ack grep <pattern> .` matched nothing | `.` is the whole repository |
| PyFinance, cbm | `ack get <directory>` was "not found" | prints the directory: files with line counts, subdirectories with file counts |
| terax | a file name shared by two files was "not found" | both files when they are small, otherwise the list of paths |
| by hand on agentsview | `Makefile` was answered with a similarly named symbol; files outside the index could not be read by name | read from the working tree |
| agentsview | Go: `Engine.processProviderFile` was "not found" (Goldeneye names it `sync.processProviderFile`) | resolved through the directory of the receiver type |
| agentsview | Go: `ack get Engine` was "ambiguous" (one struct indexed as `Class` and as `Struct`) | one symbol |
| Spring Level 0, terax | `-C` on `search` or `get` was "unknown option" | `search <pattern> [paths] -C n` runs as `grep`; `get` ignores it |

### ACK, after repetition 2 (build v6, not measured in this document)

All were seen in the recorded commands of repetition 2. Build v6 was measured
afterwards on six of the tasks; see `2026-11-12-ack-daemon-hybrid-lanes.md`.

| Seen in | Before | After |
|---|---|---|
| cbm (15 answers in one run) | a C function was "ambiguous" between its definition and its header prototype; a struct between its definition, forward declarations and mentions indexed as nested types | the definition is returned, with `# also declared: <path:line>` for the others |
| every repository | `ack get A B` exited 1 when one name was missing, which cancelled the rest of an `&&` chain and cost a turn | exit 0 when at least one name resolves; the missing one is still reported |
| agentsview, goldeneye, Spring | `Watcher.flush`, `AuthorizedPath.revalidate`, `AnnotatedBeanDefinitionReader.registerAnnotationConfigProcessors`: "did you mean" although one symbol has that member name | that symbol is shown, with a header line saying the requested owner was not found |
| agentsview | Go: a method was "ambiguous" with same-named functions of the package | the method in the receiver type's own file |
| cbm, ViV-Wallet, agentsview | `MIN_FILES_FOR_PARALLEL` (a macro), methods added since indexing, `filterTransientPaths`: "not found, try ack grep" | the declaration is located by text and shown; when none is found, the matching lines |
| terax module move, LOD | no way to apply one edit to many files; 12 calls against 6 | `ack replace <pattern> <replacement> <paths...>` (regex with `$1` groups, `-F` literal, `--dry-run`); prints each changed line |

### Goldeneye

- **Data files made indexing unusable.** LOD holds 182 MB of JSON datasets;
  every JSON key became a graph node. 4.9 MB of JSON took 41 s and 283 MB of
  database; the whole repository did not finish in 10 minutes. Discovery now
  skips data documents (`json`, `json5`, `yaml`, `toml`, `xml`, `csv`) above
  256 KB (`MAX_DATA_FILE_BYTES`, reported as `Oversized`). LOD indexes in 69 s.
  Source files of the same size are still indexed.
- **Duplicate node IDs aborted indexing** on agentsview and codebase-memory-mcp
  (`duplicate node ID in replacement`): files sharing a scope (Go files of one
  package, a C header and its source) defined the same qualified name. Such
  nodes are now merged, keeping the longest definition. Both repositories
  index (1,689 files in 91 s; 680 files in 41 s).

### Grader and harness defects

- `terax-module-refactor` failed both lanes on two test files that already fail
  at the base commit (the local `node_modules` lacks `@ai-sdk/gateway`). The
  grader now runs the same test files at the base commit and ignores those
  already failing. Regraded from the stored patches of repetition 1: vanilla
  passes, ACK fails on its type error.
- The harness set the backend's allowed root to the parent of the source
  repository, so a source repository outside the worktree root (the clean
  clones) could not be indexed. It is now the worktree root.
- `patch.diff` and the patch statistics missed changes the agent had staged
  (`git mv`, `git rm`). They are now taken against `HEAD`.
- The protocol check flagged an `ack grep` call whose pattern contained nested
  quotes as a direct source read. Fixed.
- The StringUtils configuration lacked `source_language`, so its ACK prompt
  named the wrong languages and indexed two files. Fixed before this matrix;
  earlier numbers for that task used the wrong prompt.
- After repetition 3 the ACK-lane prompt describes `ack replace`, and the
  protocol check has a test that such a call is not a direct source read. No
  run has used this prompt.

## Verification

- Goldeneye: `cargo test --workspace` 497 passed, 0 failed; `cargo clippy
  --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check`
  clean; release binary rebuilt (before repetition 2, unchanged since).
- ACK v6: `pnpm check` (lint, 177 tests, build) passes.
- Harness: `npm test` 86 passed; `npm run check` clean.
- Operation-level benchmark with ACK v6: Spring 19/19 in 20 calls, 11,536
  output tokens; goldeneye-tool 12/12 in 12 calls, 2,944 tokens. Both equal
  the v5 figures.
- ACK v6 by hand on goldeneye-tool: `ack replace <name> <new> tools/agent-bench
  --dry-run` lists 21 occurrences in 3 files and leaves the tree unchanged;
  `ack get Nope.isDirectSourceReadCommand` shows the only function of that
  name; `ack get ACK_INVOCATION_PATTERN` (not indexed) is located by text;
  `ack get <found> <missing>` exits 0.

## Not verified

- **ACK v6 is not measured in this document.** The follow-up document measures
  it on the cbm document, the 26-file move, the LOD refactor and three other
  tasks.
- The current build has two runs per task. Five of the twelve tasks change sign
  between them.
- The abyssal ACK lane of repetition 3 ran twice. The first attempt was stopped
  by the harness after 17 s, before the agent started, because the ACK source
  tree changed during preparation (`candidate provenance mismatch
  post-preparation: ack.untracked.length; expected=12 observed=13`): the v6
  source was being written while repetition 3 ran. It was rerun afterwards.
- For the same reason the ACK lanes of repetition 3 record a different ACK
  source state than repetition 2. Both executed the same built `dist` (v5,
  built 2026-11-11 22:28); the v6 source was not built until all runs ended.
- Repetition 1 mixed two Goldeneye binaries: the three ACK runs that first
  failed to start (abyssal, goldeneye rename, LOD) ran after the data-file cap;
  the other nine ran before it. The cap changes nothing for repositories
  without data files above 256 KB.
- The agentsview overlay is not compiled (no Go toolchain on this machine);
  the ViV-Wallet grader is structural (Maven cannot resolve dependencies
  offline).
- The first daemon start on the Spring index after the Goldeneye rebuild took
  116 s; the next took 1 s. Not investigated.

## Open problems

| Problem | Effect |
|---|---|
| Wide renames in the ACK lane | 12 calls against 6 on the terax move; with `ack replace` (v6) agents use it, and the move is still 2 to 3 times dearer (follow-up document) |
| Edits written from `grep` context | the LOD indentation errors (repetitions 1 and 3) |
| One more call than vanilla on a one-file edit | 22% to 28% more input on StringUtils |
| One backend start per `ack` call in the benchmark | about 2.4 s per call on a 1,689-file index; ACK is slower in wall time in every repetition |
| C struct mentions indexed as nested `Class` nodes; JSON keys indexed as `Variable` | worked around in ACK v6 for C; `ack get canonical` on LOD also lists a JSON key; LOD's index is still 310 MB |
| Go methods have no receiver in their qualified name | worked around in ACK; `callers` of a method cannot tell receivers apart |
| Inherited members | `ack get Subclass.inheritedMethod` is not found |
| Re-indexing is all or nothing | a dirty tree is read through text fallbacks until `ack index .` |
| Text-located single-line declaration holding a regex literal with brackets | the block runs on to the end of the next braced block (seen by hand on `core.mjs:135`) |

## Reproduce

```bash
cd target/agent-bench/authoring
bash run-matrix.sh a1 <task-id>...                        # both lanes
LANES=goldeneye-ack bash run-matrix.sh a1 <task-id>...
node matrix-table.mjs a 2,3 1,2,3   # ACK repetitions 2,3 against vanilla 1,2,3
node matrix-table.mjs a 3 3         # one repetition
```

`run-matrix.sh`, `matrix-table.mjs` and `regrades.json` are in the untracked
`target/agent-bench/authoring/`. Reports:
`target/agent-bench/<task>/one-shot/m-<lane>-a<n>/report.json`. Do not edit the
ACK repository while a run is being prepared; the harness aborts it.
