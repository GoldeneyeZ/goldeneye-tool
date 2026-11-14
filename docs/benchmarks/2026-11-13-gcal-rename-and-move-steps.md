# Steps and tokens: where GCAL is behind plain shell, and the rename and move case

Date: 2026-11-13. Follows `2026-11-13-gcal-final-build-three-repetitions.md`.
The measures that count are the number of steps the agent takes (tool calls)
and its input and output tokens. Wall time is an indicator and is not
optimised: the benchmark's worktrees stay on the hard disk. No agent run was
made for this note; the figures are those of the 70 runs of the final
measurement and of commands run on clones of the task repositories.

## The 70 runs again, by steps and tokens

Median of the two or three runs of each lane, GCAL / plain shell and their ratio.

| Task | Tool calls of each run, GCAL \| shell | Calls | Input | Input not cached | Output |
|---|---|---|---|---|---|
| StringUtils feature | 2,2,2 \| 2,2,2 | 2 / 2, 1.00 | 92k / 87k, 1.06 | 26k / 14k, 1.85 | 1k / 1k, 1.14 |
| terax fuzzy feature | 2,2,2 \| 3,4,3 | 2 / 3, 0.67 | 90k / 111k, 0.82 | 27k / 17k, 1.61 | 3k / 4k, 0.81 |
| abyssal feature | 3,2,3 \| 3,4,4 | 3 / 4, 0.75 | 127k / 173k, 0.74 | 20k / 21k, 0.95 | 3k / 3k, 0.84 |
| PyFinance bug | 3,3,4 \| 4,3,3 | 3 / 3, 1.00 | 136k / 193k, 0.71 | 14k / 30k, 0.48 | 3k / 3k, 1.10 |
| ViV-Wallet feature (dirty) | 4,4,4 \| 4,5,5 | 4 / 5, 0.80 | 186k / 229k, 0.81 | 17k / 23k, 0.74 | 4k / 6k, 0.71 |
| goldeneye rename (dirty) | 3,3,4 \| 2,2,3 | 3 / 2, 1.50 | 112k / 90k, 1.24 | 25k / 27k, 0.94 | 2k / 2k, 0.92 |
| terax module move | 3,4,4 \| 10,8,7 | 4 / 8, 0.50 | 148k / 314k, 0.47 | 31k / 42k, 0.74 | 4k / 8k, 0.49 |
| LOD signature refactor | 5,8,7 \| 11,3,5 | 7 / 5, 1.40 | 342k / 337k, 1.01 | 40k / 43k, 0.94 | 4k / 10k, 0.39 |
| Spring redaction feature | 7,10,7 \| 14,17,17 | 7 / 17, 0.41 | 621k / 1,264k, 0.49 | 46k / 82k, 0.56 | 9k / 15k, 0.60 |
| Spring flow question | 5,5,3 \| 7,8,11 | 5 / 8, 0.63 | 192k / 360k, 0.53 | 36k / 62k, 0.58 | 5k / 6k, 0.90 |
| agentsview flow question (dirty) | 6,6,15 \| 21,16,15 | 6 / 16, 0.38 | 381k / 1,254k, 0.30 | 79k / 116k, 0.69 | 4k / 7k, 0.63 |
| cbm architecture document | 16,10 \| 17,25 | 13 / 21, 0.62 | 672k / 1,649k, 0.41 | 78k / 104k, 0.75 | 7k / 9k, 0.73 |

Read by steps and input:

- **Two tasks are behind plain shell on both**: the goldeneye rename (one call
  more in every pair, input 1.24) and the LOD signature refactor (median 7
  calls against 5, input 1.01). Both rename a symbol in many files; LOD also
  moves it to another module.
- **StringUtils is at the floor in both lanes**: one call to read, one to
  check. The GCAL prompt is 3,275 bytes longer than the plain-shell prompt
  (5,336 against 2,061), read once per model call; that is the 1.06.
- The input that is not cached is higher with GCAL on StringUtils and on
  terax fuzzy although the calls are equal or fewer. The reports give one
  figure per run, not per model call, so what the provider cached of which
  call cannot be read from them. Not explained.
- Everywhere else GCAL takes fewer steps and less input.

## What the rename and move runs spend their steps on

Tool calls before the first edit, with the bytes each returned.

| Run | Calls before the first edit |
|---|---|
| goldeneye rename, GCAL f1 and f2 | `get RevalidatedPath.as_path --tests` (1,600), `grep 'as_path' crates docs -C 2` (3,618) |
| goldeneye rename, GCAL f3 | `get ... --tests` (3,937), `callers RevalidatedPath.as_path` (1,300), `grep` (3,618) |
| goldeneye rename, shell, all three | one `rg` of the two names (2,484 to 6,395); f3 also lists a directory |
| LOD, GCAL f1 | `get canonical strict_json --tests` (16,104), `callers canonical; grep '\bcanonical\b' . -C 1` (38,625) |
| LOD, GCAL f2 | `get ... --tests` (16,104), `callers canonical` (10,623), `grep` of the import lines (3,522) |
| LOD, GCAL f3 | `get ... --tests` (11,588), `grep '\bcanonical\b' src tests scripts -C 2` (22,712), `grep` of the import lines (3,246) |
| LOD, shell, all three | `rg` of the name in `*.py` (13,690 to 13,932), `Get-Content` of the two modules (9,170) |

Three causes, each seen in the traces:

1. **The first call is `get --tests` in all six GCAL runs.** The skill and
   the benchmark prompt say to put `--tests` on "the symbols you will change".
   For a rename the tests of the symbol are not what is needed, every use of
   it is; the call returns the source and tests (up to 16 kB on LOD), and the
   uses still have to be asked for.
2. **`gcal callers <symbol>` did not list all a rename changes.** It listed
   calls. It left out the declaration, the import statements (38 on LOD, which
   is why two of three runs asked a second `grep` for them; three of the 38
   are written inside a function, indented, where a replacement anchored with
   `^` does not reach them), the places that name the symbol in full without
   calling it (`RevalidatedPath::as_path` in a doc comment and in the Markdown
   documentation), and every use in a file created since indexing (a test
   file of the dirty tree). So the agent that asked `callers` asked `grep` as
   well.
3. **Leftovers are repaired from whole files.** LOD f2 replaced the bare word
   `\bcanonical\b`, which also changed prose in four docstrings; it then read
   five files whole (23,274 bytes) and searched again. LOD f3 read three
   files whole (32,378 bytes) to patch what its two replacements had left in
   them. Every byte read early is read again by each later model call.

## What was changed

**`gcal callers <symbol>` now answers a rename in one call.**

- A `def` row for the declaration, located in the file as it is now.
- For a function, how the files import it: the statement most files write,
  with its count and the places where it is written inside a function, then
  every other spelling with its place. Statements over several lines are read
  (`from m import (` … `name,`). On LOD:

  ```
  # callers of canonical: 60 (graph 57, text-only 3)
  # imported in 38 files, 37 times as `from lod.training.pairs import canonical` (3 indented, inside a function: src/lod/training/learned_codec/diagnostics/engine_probe.py:27, training/colab/colab_decoder_thinking.py:35, training/experiments/batch-capacity/probe.py:153)
  #   src/lod/training/pilot.py:14	from lod.training.pairs import canonical, load_pairs, tokenize_pair
  src/lod/training/pairs.py
    9	canonical	def	def canonical(value):
    86	load_pairs	call	        content = canonical(target)
  ```

- For a member, the lines that name it in full without calling it
  (`Owner::member`, `Owner.member`, `Owner#member`), as `text` rows: doc
  links, documentation. Documents outside the index are read for these.
- Calls and mentions in files created since indexing.

**The skill and the benchmark prompt** say: `--tests` is for a symbol whose
behaviour changes; a rename, a move or a signature change starts with
`gcal callers <symbol>`; leftovers of a replacement are patched from the rows
already printed or from a range, not from whole files; a bare word as
replacement pattern also changes prose.

## Verification, without a model

- LOD, on a clone at the task's base commit: the answer of
  `gcal callers lod.training.pairs.canonical` is 11,037 bytes (10,627 before).
  A scan of the Python files finds 38 import lines naming the function, 37 of
  one spelling, 3 indented: the answer says the same. Each of the three
  recorded patches that passed the grader (GCAL f1, GCAL f3, shell f3)
  removes 113 lines naming `canonical`: 37 are that import statement, and all
  76 others are at a place the answer lists. The three discovery calls of
  LOD f2 returned 30,249 bytes for the same information.
- goldeneye rename, on a clone: with a doc link, a line of Markdown and a new
  test file added by hand, the answer lists the declaration, the doc link,
  the Markdown line, and the call and the link in the new file.
- `pnpm check` in `agent`: 25 files, 246 tests, none failed (7 added: the
  import statements of five languages, lists over several lines, lines that
  are not imports; the two new parts of the `callers` answer). `npm test` in
  `tools/agent-bench`: 88 tests, none failed.

## One agent run of the three tasks (2026-11-14)

Build `1f4592b`, GCAL lane only, one run per task (attempt `m-callers-c1`),
462k tokens in total. One run is not a median; the earlier runs of both lanes
are beside it for comparison and were made the day before.

| Task | Passed | Calls | Input | Input not cached | Output |
|---|---|---|---|---|---|
| goldeneye rename, this run | yes | 2 | 66k | 25k | 2.2k |
| earlier GCAL runs | 3 of 3 | 3, 3, 4 | 112k, 112k, 140k | 9k, 25k, 30k | 1.7k, 1.5k, 2.1k |
| earlier shell runs | 3 of 3 | 2, 2, 3 | 90k, 84k, 135k | 28k, 23k, 27k | 1.9k, 1.6k, 2.9k |
| LOD signature refactor, this run | **no** | 5 | 232k | 38k | 4.2k |
| earlier GCAL runs | 3 of 3 | 5, 8, 7 | 242k, 348k, 342k | 29k, 47k, 40k | 3.5k, 4.0k, 4.4k |
| earlier shell runs | 2 of 3 | 11, 3, 5 | 563k, 164k, 337k | 51k, 20k, 43k | 11.2k, 6.6k, 10.3k |
| terax module move, this run | yes | 4 | 151k | 23k | 6.4k |
| earlier GCAL runs | 3 of 3 | 3, 4, 4 | 128k, 149k, 148k | 31k, 23k, 33k | 3.3k, 4.3k, 3.8k |
| earlier shell runs | 2 of 3 | 10, 8, 7 | 592k, 265k, 314k | 50k, 27k, 42k | 8.6k, 7.7k, 7.0k |

What the traces show:

- **All three runs start with `gcal callers`**, none with `get --tests` alone,
  and none asks `grep` for the uses afterwards.
- **goldeneye rename**: `callers RevalidatedPath.as_path` (1,796 bytes), then
  one shell call with the replacements. Two calls, the floor of the
  plain-shell lane, with less input than any of the six earlier runs.
- **LOD**: `callers` (11,033 bytes), `get` of the two modules with `--tests`
  (12,501), a replacement of the bare word `\bcanonical\b` (122 occurrences
  in 43 files, prose included) that the next call undoes before replacing by
  pattern, then `check`. Five calls, and the run **failed the grader**: the
  hand patch that moved the function out of `pairs.py` also removed
  `import json`, which two other lines of the file use
  (`NameError: name 'json' is not defined`). `gcal check` had run and had
  reported another fault of the same run (a replacement written with `\n`,
  which put the two characters in the file instead of a line break), but not
  this one. The three earlier GCAL runs of this task passed; with one run it
  cannot be said whether the new first call changes the pass rate.
- **terax module move**: four calls like before; input the same, output higher
  (6.4k against 3.3k to 4.3k). The third call replaces the bare word `native`
  in a list of files.

### What the run led to

- **`gcal check` names what a file still uses after the change removed its
  import**, from the text of HEAD and of the working tree (Python, JavaScript
  and TypeScript, Java and Kotlin, Rust). With the failed patch applied to a
  clone of LOD:

  ```
  # syntax check: 41 changed files parse
  # an import the change removed, and a name the file still uses:
  src/lod/training/pairs.py: `import json` is gone, `json` is still used
    12	        json.loads(line)
    75	                    json.loads(item["arguments_raw"], parse_constant=reject_constant)
  ```

  Run over every recorded patch of the eleven tasks that have a clone (245
  patches, 2,411 edited files): two findings, this one and the same line in an
  earlier failed LOD run (`m-hybrid-b2`); none on a run that passed.
- **`gcal replace` writes a line break for `\n` in a regex replacement** (and
  a tab for `\t`), with the line ending of the file; inside a quoted span of
  the replacement, and with `-F`, the two characters stay as typed. Of the
  recorded replacements, the two that contain `\n` both meant a line break.
- **The benchmark prompt** now says, like the skill, that a bare word as
  pattern also rewrites comments and strings. Of the four recorded LOD runs of this
  lane on the final build and after, two (f2 and this one) began with the bare
  word.
- `pnpm check`: 26 files, 254 tests, none failed (8 added). `npm test` in
  `tools/agent-bench`: 88 tests, none failed.

## Found and not changed

- **The call graph has edges that are not calls of the symbol.**
  `callers RevalidatedPath.as_path` lists four lines of `xtask` that call
  `as_path` of a standard-library path (6 graph rows, 3 of them
  wrong). The files of the true callers name the module of the type
  (`path_auth`); those of `xtask` name neither the type nor its module. No
  recorded run renamed them, but the answer does not say which rows are in
  doubt, and `gcal check` reads the same graph.
- **`gcal replace` changes a bare word in comments and strings like anywhere
  else.** Only the text of the skill warns about it now.
- **The GCAL prompt of the benchmark is 3.3 kB longer** than the plain-shell
  one, on every model call. It grew again with this change.

## Not verified

- One agent run per task was made with the `callers` change, none with what
  followed it (the import check, `\n` in a replacement, the prompt's sentence
  on bare words). Whether LOD passes with them, and in how many calls, is not
  measured; neither is a median of the three tasks. The plain-shell lane was
  not run again. About 0.15M tokens are left of the budget granted for the
  final measurement (26,850k of 27M used), less than one LOD run.
- The import check reads statements by their text. It was run on the recorded
  patches (Python, TypeScript, Java, Rust files among them) and found no
  wrong report there; a name bound in a way it does not read (a parameter of
  an arrow function over several lines, a `global`) can still give one.
- The Rust and Java import statements are covered by unit tests on their
  text only; the import summary was run on one Python repository.
