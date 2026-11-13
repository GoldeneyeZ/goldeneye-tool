# GCAL improvement goal, round 9: lambdas and package lines in the index, the layout, the slow pre-warm

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-8.md`.

## The point this round works on

The replay of round 8 had shown the index's word could not be taken for what
is a declaration: a Java lambda `csrf -> csrf.disable()` was indexed as a
function named `csrf`, and the first version of the check for removed symbols
reported it in eleven passing runs. The check was made to confirm declarations
by text; the index itself was left wrong. It is the same class of defect as
the JavaScript callbacks fixed earlier in this goal (`d254f7e`), in every other language.

## What was changed

**Index: a lambda or closure is not a symbol of its own** (commit `caa9bc0`).
An anonymous function (`lambda_expression`, `closure_expression`,
`func_literal`, `arrow_function` and six more node kinds) was named after the
first identifier inside it. It now has a name only where the language gives it
one: in JavaScript and TypeScript the binding or property it is assigned to,
in Nix the binding that holds it (every Nix function is written this way).
Elsewhere it is no symbol, and the calls inside it belong to the function
around it.

Callables in the index, before and after, on repositories indexed again with
the new binary:

| Repository | Before | After |
|---|---|---|
| Spring Framework, three modules | 31,791 | 28,689 |
| goldeneye-tool, Rust | 4,502 | 2,958 |
| abyssal-zenith | 1,214 | 995 |
| ViV-Wallet | 393 | 339 (12 arrow functions keep the name they are assigned to) |

**`gcal arch`: directories of source are opened before directories of
documents** (commit `06f7d2a`). Found by the operations benchmark, not looked
for: its corpus is this repository, which was indexed again for the change
above and had grown from 722 to 919 indexed files, 299 of them Markdown. The
layout opened the largest directory first, spent its 40 rows on the notes
under `docs/`, and printed `ui/` (77 files) and `tools/` (111) as one row
each; the overview scenario failed for want of `ui/src`. Directories are now
opened in the order of the source files they hold.

**`replay-check.mjs --reindex`** (commit `e43e92a`): indexes the replay clones
again, for a changed backend binary.

## Verification without a model

- The 159-language corpus passes with a new test: a Java lambda and a Rust
  closure are not symbols, and their calls belong to the enclosing function.
  The first version of the rule failed that corpus for Nix, where it removed
  every function; hence the Nix rule.
- `cargo test --workspace` (101 test binaries), `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --all --check`: pass.
- Replay of the recorded patches on clones indexed again with the new binary:
  49 runs, the same verdicts as before. Of the 5 that failed their grader the
  check reports 3; of the 44 that passed, none.
- Operations benchmark, both corpora indexed again from scratch (Spring 64 s,
  goldeneye-tool 44 s):

  | Corpus | GCAL before | GCAL after | Plain shell |
  |---|---|---|---|
  | Spring Framework | 19 of 19, 20 calls, 11,735 tokens | 19 of 19, 20 calls, 11,735 tokens | 19 of 19, 21 calls, 14,900 tokens |
  | goldeneye-tool | 12 of 12, 12 calls, 2,906 tokens | 12 of 12, 12 calls, 2,978 tokens | 12 of 12, 13 calls, 4,250 tokens |

  On goldeneye-tool the run between the two fixes was 11 of 12 (the layout
  scenario). The token count of that corpus is not comparable across the
  re-index: the repository it indexes is this one and has changed.
- `pnpm check` passes (226 tests, one new for the layout); the harness tests
  pass (88).

## Agent run

One run, the last the budget allowed: the StringUtils feature on the final
build (`06f7d2a`, new binary, Spring index rebuilt by the harness). It passed:
91k input tokens, 2 tool calls, 35 s (the five earlier runs of this task:
89k to 110k, 2 calls, median 44 s; plain shell 86k, 2 calls, 40 s). The agent
ran `gcal get 'StringUtils.truncate' 'StringUtilsTests.java' --tests`, one
patch, then `gcal check; git status --short`. This shows the build works end
to end on a Java repository indexed with the new rule; it does not measure the
rule, since the task reads no caller.

Token cost of the round: 92k. Cumulative: 14,928k of the 15M budget. The
remaining 72k are less than any task of the matrix costs.

## Where the pass rate stands

All runs of this goal: 56 of 61 (plain shell: 42 of 43). The seven runs made
after the last fixes for the failures all passed. The clause "same or better
pass rate" is not met by measurement and cannot be with the budget left.

## The harness's slow daemon pre-warm, explained

Unexplained in the summary until now: before the prompt the harness runs `gcal status` through
the daemon and records the time. Per task, in the runs of this goal:

| Task | Pre-warm, seconds |
|---|---|
| StringUtils (Spring) | 8.7 to 11.8 |
| Spring redaction | 8.3 to 9.8 |
| LOD | 8.2 to 11.5 (once 2.4) |
| cbm | 7.9 to 9.3 |
| Spring flow question | 3.3 to 7.3 |
| agentsview | 4.2 to 4.5 |
| the six small repositories | 0.9 to 4.4, mostly 1.4 |

The same command took 1.0 s outside the harness on the same index. The cause
is the disk the benchmark works on, not GCAL. The harness's worktrees and live
caches are on `D:`, a SATA hard disk (TOSHIBA DT01ACA300); `git worktree add`
writes 10,986 files of Spring there in 45 to 57 s, and the pre-warm is the
first thing to write to that disk afterwards (the snapshot copy, then SQLite's
`-wal` and `-shm` files when the index is opened).

Measured by replaying the harness's own sequence for the StringUtils task
without a model (worktree at the harness's path, snapshot restored, `gcal
status` with the lane's environment), the worktree removed afterwards as the
harness does:

| Index and GCAL state restored to | Snapshot copy (129 MB) | `gcal status`, cold daemon | Second `gcal status` |
|---|---|---|---|
| `C:` (NVMe) | 0.08 s | 1.2 s | 2.3 s |
| `D:` (the hard disk), right after `worktree add` | 32 s | 29 s | 36 s |
| `D:`, once the disk had settled (direct backend) | | 0.85 s | |

The second call waits behind the daemon's graph warm-up, which reads the
index. Ruled out on the way, by measurement: changed modification times of
the worktree (no effect), the harness's environment variables (no effect), a
freshly copied index on a quiet disk (1.1 s). By reading the code: the
daemon's start lock (a wait there ends in a fallback after 2 s, not in a slow
answer) and the backend's file watcher (GCAL disables it).

Does the same contention slow the plain-shell lane, which has no pre-warm and
starts its agent sooner? Reads of the new worktree are served from memory:
8 s after `worktree add`, `rg` over the three Spring modules took 380 ms and
`git status` 102 ms; 30 s later 314 ms and 86 ms. So the contention is on new
writes, the pre-warm takes it, and the pre-warm is not part of the wall time
compared between the lanes (`wall_ms` is the agent's time only). What was not
measured is the end of a run, where both lanes write (patches) on that disk.

## An audit of the index's names, and what it found

With no tokens left, the next point was taken from the index itself: after
lambdas, what else does it call by a name nobody wrote? Three scripts read the
SQLite index of seven repositories indexed with the new binary (abyssal, LOD,
PyFinance, Spring's three modules, terax, ViV-Wallet, goldeneye-tool) next to
the files.

| Question | Result |
|---|---|
| Is the symbol's name written in the first 8 lines of its span? | 174,882 symbols; 66 are not, of which 46 are Java declarations under a long annotation, 18 uses of a zsh variable and 2 YAML anchors |
| Is the name one identifier? | 183,434 symbols; 94 are a JavaScript or TypeScript destructuring pattern taken whole (`{ rows }`, `[isOpen, setIsOpen]`). Left as they are: the search finds them by either name. |
| Is a type or function declared on a `package`, `import` or `namespace` line? | **2,920 Java classes are `package` lines**, named after the first segment of the package: one `org` per file in Spring (2,614 of its 8,549 classes), one `fr` per file in abyssal |

The third is a defect of the Java language rules as they were
written: `package_declaration` is listed among the kinds that declare a class.
It was visible in the operations benchmark, in an answer that passed
its scenario: `gcal search 'MergedAnnotations|TypeDescriptor' --label Class`
returned five rows, two of them `org Class ...MergedAnnotations.java:17
package org.springframework.core.annotation;`.

**Fix** (commit `de7ad90`): a Java `package` line declares no symbol.

Verification without a model:

- A new corpus test, seen failing before the change (`org` was two classes
  in a two-file fixture) and passing after it; it also asserts the call from
  one file to a class of another package is still linked. The 159-language
  corpus, `cargo test --workspace` (101 test binaries), clippy and fmt pass.
- Spring's three modules indexed again: 50,474 nodes become 47,860, which is
  2,614 fewer, the number of files.
- Operations benchmark: Spring 19 of 19, 20 calls, 11,666 tokens (before:
  11,735; the one scenario that changed is the search above, 198 tokens to
  129); goldeneye-tool 12 of 12, 2,978 tokens, unchanged.
- Replay of the 16 recorded runs of the three Java tasks on clones indexed
  again: the same verdicts (of the 2 that failed their grader the constructor
  call is reported, the nullness error is not; of the 14 that passed, none is
  reported).

No agent run was made on this binary.

### The same audit on a Go and a C repository

agentsview (934 Go files, 456 TypeScript) and codebase-memory-mcp (420 C
files, 87 headers) were indexed into scratch state, their checkouts untouched,
and asked the same questions plus one more: is a type's name written on the
first line of its declaration, and do two type symbols of the same name start
on the same line?

| Finding | Count | Effect on an answer |
|---|---|---|
| Go: every `type X struct` is two symbols, a class for the declaration and a struct for the specification | 1,563 of the 1,693 classes of agentsview | `gcal search resumeResponse` and `gcal symbol` returned two rows for one type |
| C: a struct or union without a tag is a class named after its first field, or after that field's type | 447 of the 776 classes of codebase-memory-mcp by the first-line test; 463 classes fewer after the fix | `typedef struct { FILE .. } PageBuilder;` was a class `FILE`; `CBMCallArray` was a class `CBMCall` |
| TypeScript: a class expression without a name is a class named after its first method | 10 | `observe` as a class |

**Fix** (commit `b2b3fac`): the declaration around a Go type
specification is no symbol; a C-family struct, union or class without a tag is
no symbol (the `typedef` keeps it under its real name); a JavaScript class
expression without a name is no symbol.

The first version of the C rule also covered enums and was taken back for
them after the index was read again: it removed 252 of the 256 enums of
codebase-memory-mcp, because C code writes constants as `enum { LIMIT = 8 };`
and the first enumerator's name is how such a constant is found.

Verification without a model:

- A new corpus test (Go, C, TypeScript), seen failing before the change. The
  corpus (12 tests, 159 languages), `cargo test --workspace`, clippy and fmt
  pass.
- The two repositories indexed again. Classes: agentsview 1,693 to 124,
  codebase-memory-mcp 776 to 313. Call edges: agentsview 41,306 to 41,382,
  codebase-memory-mcp 37,486 to 37,486. Structs of agentsview: 1,423 to
  1,420; the three that went were not looked up.
- Operations benchmark with the final binary: Spring 19 of 19, 11,666 tokens;
  goldeneye-tool 12 of 12, 2,978 tokens; no scenario changed.
- Replay of 30 recorded runs of five tasks (TypeScript, Rust, Java) on clones
  indexed again: the same verdicts (3 failed their grader, 2 of those are
  reported; 27 passed, none reported).

Seen and left: PowerShell type references (`[string]`, `[System.Environment]`)
are classes (38 in the two repositories); every key of a JSON file is a
variable (13,055 in agentsview, its message catalogs among them).

No agent run was made on this binary either. The two tasks on these
repositories are the most expensive of the matrix (468k and 754k tokens per
run with GCAL).

### What the keys and headings do to a search

Every key of a JSON file and every Markdown heading is a symbol. On
agentsview, which has 4 JSON message catalogs and 89 indexed Markdown
files, a search for a plain word was answered with those:

| `gcal search <word> --limit 20` on agentsview | Rows from JSON and Markdown, before | After |
|---|---|---|
| `session` | 19 of 20 (9 keys, 10 headings) | 6 of 20; first row `db.Session Struct` |
| `resume` | 13 of 20 | 7 of 20 |
| `sync` | 2 of 20 | 0 of 20 |
| `usage` | 8 of 14 | 8 of 14 (the page was not full: nothing was hidden) |

**Fix** (commit `5deea6d`): `search` lists the symbols of code before rows
from prose and data files, and when the backend's first page is full and
holds fewer code symbols than the limit it fetches the largest page once. One
unit test for the order and the second fetch; `pnpm check` passes (227
tests); the operations benchmark is unchanged on both corpora (19 of 19,
11,666 tokens; 12 of 12, 2,978 tokens); the harness tests pass (88).

Not measured: whether the agents of the agentsview task ran such searches.
Their recorded traces were not read for this.

## Limits

- The callable counts are totals; no list of the removed symbols was reviewed
  beyond the samples that led to the change (Java lambdas, Rust closures) and
  the corpus.
- A caller that was linked to a lambda's invented name is now linked to the
  enclosing function. No recorded run depended on such a row, so the replay
  cannot show the effect on `callers` answers.
- The prose rule of `arch` is a list of extensions (`md`, `mdx`, `markdown`,
  `txt`, `rst`, `adoc`). It was checked on two repositories.
