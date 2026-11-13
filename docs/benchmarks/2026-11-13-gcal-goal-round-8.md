# GCAL improvement goal, round 8: a check for what the failed runs left behind

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-7.md`.

## The point this round works on

After seven rounds GCAL is cheaper than plain shell and passes less often: 53
of 58 runs against 42 of 43. All five failures are changes that parse and do
not build:

| Run | What the grader found |
|---|---|
| goldeneye rename, round 5 | `path_auth.rs:153`: old name `as_path` still used (`self.as_path()`) |
| goldeneye rename, round 6 | `inspection.rs:87` and `:90`: old name `as_path` still used |
| Spring redaction, round 6 | `DataBinderFieldAccessTests.java:187`: no suitable constructor found for `MutablePropertyValues(String,String)` |
| Spring redaction, round 5 | `DefaultBindingErrorProcessor.java:80`: NullAway, `@Nullable` passed where `@NonNull` is required |
| terax fuzzy, round 5 | existing tests fail: `q.length` on a value that became an object |

The agents end their edits with `gcal check`, which parsed the files and said
they were fine.

## What was built

Commit `f191eba`. `gcal check` without paths now also reports, from what the
index knows:

1. **A symbol that HEAD declares and the working tree no longer does, whose
   callers still name it.** The callers come from the call graph. The graph
   links a call to a method by name, so for a method a caller's file counts
   only if it names the method's type or calls a function whose signature names
   it; without that rule every `.as_path()` on a standard path buffer was
   reported.
2. **A deleted file that is still imported.**
3. **In Java, `new Type(..)` on an added line** when `Type` is one class of
   the project, declares constructors in its source, and none takes that
   number of arguments. Classes with constructor-generating annotations and
   variable-argument constructors are left alone.

It is not a compiler: it sees names and argument counts, not types.

## Verification without a model: replaying the recorded patches

Every agent run leaves its final diff. For nine of the twelve tasks (the three
others produce an answer or a document, not a code change) each recorded diff
of this goal was applied to a clone of the repository at the task's base
commit, indexed at that commit, and `gcal check` was run on the result.

| | Patches | Check reports something |
|---|---|---|
| Runs that failed their grader | 5 | 3: the two renames and the constructor call, each time with the line the grader named |
| Runs that passed their grader | 42 | 0 |

The two failures it does not report are the nullness error and the type error.

Two versions of the first rule were rejected by this replay before the one
that was kept: without the type rule it reported four standard `as_path()`
calls in every rename run, passing or not; and taking the index's word for
what is a declaration it reported a Java lambda (`csrf -> csrf.disable()`,
indexed as a function named `csrf`) and the members of an object literal as
removed symbols in eleven passing runs.

## Agent runs

Two runs of the rename task on the commit with the check, the last of the
budget: both passed (95k and 113k input tokens, 3 calls each; plain shell
116k, 3 calls). In neither did the agent leave anything behind, so the check
had nothing to report and these runs do not show an agent acting on a report.
That is not measured.

Token cost of the round: 212k. Cumulative: 14,836k of the 15M budget. The
remaining 164k are not spent.

## Where the pass rate stands

- All runs of this goal: 55 of 60 (plain shell: 42 of 43).
- The six runs made after the last fixes for the failures (rounds 7 and 8, on
  five tasks, the Spring redaction feature and the rename among them): 6 of 6.
  Six runs on five tasks do not establish a pass rate.
- By construction of the replay, three of the five failed changes would have
  been reported to the agent by the `gcal check` it ran at the end of that
  very run. Whether it would then have fixed them is not known.

## After the runs: two more changes, verified without a model

No tokens were left to measure these with agents.

**`gcal callers` on a file changed since indexing** (commit `5e7c304`). A call
the graph knows was printed with the line of the index and no text; in the
rename task's dirty tree that was `141 RevalidatedPath.as_ref call` followed by
nothing, next to a separate row for the real line. The call is now found again
in the caller as the working tree has it. On a clone of that repository with
the overlay applied the answer has six rows, each with its line and text, the
one in question being `153 RevalidatedPath.as_ref call self.as_path()`.

**`gcal check --types`** (commit `ada6a1e`). The two failures the index cannot see
are type errors; only a compiler sees those. With `--types` the check runs the
project's own type checker, TypeScript only: `tsc --noEmit` from the project's
`node_modules`. When that reports errors, HEAD is exported to a temporary
directory and type-checked as well, and only the errors HEAD does not have are
printed. It is off by default and the lane prompt does not mention it: it is a
build step, which the one-shot rule forbids to both lanes.

Replayed on the 12 recorded patches of the two terax tasks, with the project's
dependencies linked in as the harness does:

| Patch | Grader | `gcal check --types` |
|---|---|---|
| terax fuzzy, round 5 | failed | 1 new error: `fuzzy.ts:65 TS2339 Property 'length' does not exist on type '{ chars: string[]; indices: number[]; }'`, the cause of the failure |
| terax fuzzy, round 6 run b | passed | 6 new errors in `fuzzy.ts` (`Type 'string' is not assignable to type 'FoldedCharacter'` and five comparisons without overlap). The grader of this task runs the tests, which do not type-check; the patch does not pass the project's `tsc --noEmit`. |
| the other 10 (5 fuzzy, 5 module move) | passed | no new error |

The project has 61 type errors at the task's base commit with the installed
dependencies; the comparison with HEAD removes them from every answer. One
check takes 25 to 30 s (two compiler runs).

With it, four of the five failed changes are reported by a command the agent
could run; the fifth needs the Spring build's nullness checker.

## Fairness

`gcal check` is the one command the one-shot rule allows the GCAL lane after
editing; plain shell has none. Until this round it parsed files. It now also
compares the change with the call graph. It still runs no compiler, no build
and no test of the project, but the gap between what the two lanes may learn
before grading is wider than before, and a comparison of pass rates between
the lanes has to say so.

## Checks

- `pnpm check` passes (225 tests; 4 for the index-based check, 3 for the type
  check, 1 for the re-found call); the harness tests pass (88).
- Operations benchmark: Spring 19 of 19, 11,735 tokens; goldeneye-tool 12 of
  12, 2,906 tokens.

## Limits

- The check was shaped on the same five failures it is evaluated on. The 42
  passing patches are the only evidence against false reports, and they come
  from nine tasks.
- The constructor rule is Java only and counts arguments; a call with the
  right number of wrongly typed arguments passes it.
- The caller rule depends on the call graph of the index at HEAD; a caller the
  graph does not know is not reported.
