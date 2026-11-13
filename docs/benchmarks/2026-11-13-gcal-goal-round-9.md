# GCAL improvement goal, round 9: lambdas in the index, the layout of a repository with many notes

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

## Limits

- The callable counts are totals; no list of the removed symbols was reviewed
  beyond the samples that led to the change (Java lambdas, Rust closures) and
  the corpus.
- A caller that was linked to a lambda's invented name is now linked to the
  enclosing function. No recorded run depended on such a row, so the replay
  cannot show the effect on `callers` answers.
- The prose rule of `arch` is a list of extensions (`md`, `mdx`, `markdown`,
  `txt`, `rst`, `adoc`). It was checked on two repositories.
