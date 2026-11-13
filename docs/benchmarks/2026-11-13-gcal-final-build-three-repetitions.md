# GCAL final build against plain shell: three repetitions on one day

Date: 2026-11-13. Follows `2026-11-13-gcal-goal-round-9.md` and settles two
points `2026-11-13-gcal-goal-summary.md` had left open: the final build had
never been measured as such, and the plain-shell numbers were one to two days
older than the GCAL numbers they were compared with.

## What was run

70 agent runs between 10:02 and 14:11: the twelve-task matrix, GCAL (hybrid
lane, daemon on) and plain shell, one pair per task and repetition, the GCAL
run first and the plain-shell run right after it. Each run is one Codex agent
(`gpt-5.6-terra`, reasoning high) graded by a held-out grader; neither lane may
build or run tests.

| Repetition | Time | Build | Tasks | Runs |
|---|---|---|---|---|
| f1 | 10:02 to 11:32 | `2142804` | 12 | 24 |
| f2 | 11:33 to 12:30 | `2142804` | 11 (all but the Spring redaction feature) | 22 |
| f2, Spring redaction feature | 12:38 to 12:58 | `6062619` | 1 | 2 |
| f3 | 12:58 to 14:11 | `6062619` | 11 (all but the cbm document) | 22 |

Two things in that table are not as planned. The Spring redaction pair of f2
was held back because the GCAL run of f1 had failed on it; it was run after
the fix described below. The cbm document pair of f3 was not run: the budget
for these runs was about 27M tokens, 26,388k were spent after f3, and a cbm
pair had cost 2,274k and 2,367k. That task has two repetitions, the others
three.

The two builds differ in one thing: `gcal check` on `6062619` has the nullness
rule for Java. The backend binary is the same.

## Result

Median of the repetitions per task; "F" marks a run that failed its grader.

| Task | GCAL pass | GCAL input per run | median | calls | wall | Shell pass | Shell input per run | median | calls | wall | Input ratio | Wall ratio |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| StringUtils feature | 3/3 | 92k, 92k, 90k | 92k | 2 | 45 s | 3/3 | 89k, 85k, 87k | 87k | 2 | 39 s | 1.06 | 1.15 |
| terax fuzzy feature | 3/3 | 116k, 90k, 90k | 90k | 2 | 86 s | 3/3 | 111k, 142k, 110k | 111k | 3 | 123 s | 0.82 | 0.70 |
| abyssal feature | 3/3 | 127k, 99k, 130k | 127k | 3 | 79 s | 3/3 | 126k, 173k, 175k | 173k | 4 | 92 s | 0.74 | 0.86 |
| PyFinance bug | 3/3 | 136k, 126k, 157k | 136k | 3 | 82 s | 3/3 | 196k, 156k, 193k | 193k | 3 | 78 s | 0.71 | 1.05 |
| ViV-Wallet feature (dirty tree) | 3/3 | 189k, 186k, 184k | 186k | 4 | 117 s | 3/3 | 157k, 229k, 263k | 229k | 5 | 149 s | 0.81 | 0.78 |
| goldeneye rename (dirty tree) | 3/3 | 112k, 112k, 140k | 112k | 3 | 53 s | 3/3 | 90k, 84k, 135k | 90k | 2 | 54 s | 1.24 | 0.99 |
| terax module move | 3/3 | 128k, 149k, 148k | 148k | 4 | 111 s | 2/3 | 592k, 265k F, 314k | 314k | 8 | 198 s | 0.47 | 0.56 |
| LOD signature refactor | 3/3 | 242k, 348k, 342k | 342k | 7 | 134 s | 2/3 | 563k, 164k F, 337k | 337k | 5 | 265 s | 1.01 | 0.50 |
| Spring redaction feature | 2/3 | 621k F, 668k, 354k | 621k | 7 | 269 s | 3/3 | 1,264k, 1,754k, 932k | 1,264k | 17 | 415 s | 0.49 | 0.65 |
| Spring flow question | 3/3 | 229k, 192k, 131k | 192k | 5 | 148 s | 3/3 | 360k, 318k, 480k | 360k | 8 | 162 s | 0.53 | 0.91 |
| agentsview flow question (dirty tree) | 3/3 | 381k, 337k, 933k | 381k | 6 | 130 s | 3/3 | 1,514k, 1,254k, 1,208k | 1,254k | 16 | 197 s | 0.30 | 0.66 |
| cbm architecture document | 2/2 | 915k, 429k | 672k | 13 | 212 s | 2/2 | 1,359k, 1,938k | 1,649k | 21 | 264 s | 0.41 | 0.80 |
| **Sum of medians** | **34/35** | | **3,101k** | **59** | **1,467 s** | **33/35** | | **6,062k** | **94** | **2,036 s** | **0.51** | **0.72** |

Sums over all 35 runs of each lane:

| | GCAL | Plain shell | Ratio |
|---|---|---|---|
| Runs passed | 34 of 35 | 33 of 35 | |
| Input tokens | 8,819k | 17,218k | 0.51 |
| of which not cached | 1,213k | 1,621k | 0.75 |
| Output tokens | 143k | 208k | 0.69 |
| Tool calls | 173 | 270 | 0.64 |
| Wall time of the agent | 4,236 s | 5,763 s | 0.74 |
| Whole harness invocation, preparation and grading included | 7,201 s | 7,161 s | 1.01 |

Over the eleven tasks that have three repetitions in both lanes (33 pairs):
7,475k against 13,920k input (0.54), 147 calls against 228, 3,812 s against
5,236 s (0.73).

Pair by pair (35 pairs of one GCAL run and the plain-shell run after it): GCAL
took fewer input tokens in 24, fewer uncached input tokens in 25, less wall
time in 28; fewer tool calls in 22, the same number in 7, more in 6. Both runs
of the pair passed in 32.

By repetition:

| Repetition | GCAL pass | Shell pass | GCAL input | Shell input | Ratio | Calls | Wall |
|---|---|---|---|---|---|---|---|
| f1, 12 tasks | 11/12 | 12/12 | 3,289k | 6,420k | 0.51 | 59 against 98 | 1,489 s against 2,101 s |
| f2, 12 tasks | 12/12 | 10/12 | 2,829k | 6,563k | 0.43 | 59 against 97 | 1,482 s against 1,989 s |
| f3, 11 tasks | 11/11 | 11/11 | 2,701k | 4,235k | 0.64 | 55 against 75 | 1,265 s against 1,673 s |

## Pass rate

GCAL 34 of 35, plain shell 33 of 35, on the same tasks, the same day and in
alternation. This is the comparison the goal asked for ("same or better pass
rate"), and on it GCAL is one run ahead. One run is not a difference that
35 runs per lane can establish: what the numbers support is that the two
lanes pass at the same rate, not that GCAL passes more often.

On build `6062619` alone: 12 of 12 for each lane. On `2142804`: 22 of 23
against 21 of 23.

## The three failed runs

**GCAL, Spring redaction feature, f1.** The agent wrote

```java
final Object getRejectedValue(String field, @Nullable Object rejectedValue) {
	return (isSensitiveField(field) ? REDACTED_VALUE : rejectedValue);
}
```

in a package that is non-null by default. The grader builds with NullAway,
which rejects it: a `@Nullable` value is returned from a method whose return
type is not `@Nullable`. `gcal check` had answered "6 changed files parse".
This is the failure class the summary listed as invisible to the check.

**Plain shell, terax module move, f2.** 24 of 25 references to the moved
module were rewritten; `src/modules/ai/lib/transport.ts:6` still imports
`./native`.

**Plain shell, LOD signature refactor, f2.** 74 of 75 call sites were
rewritten; `src/lod/training/learned_codec/diagnostics/base_capability_probe.py:66`
still calls `.canonical`. This run is also the cheapest of its task (164k,
3 calls): it stopped early.

Both plain-shell failures are one reference left behind by a rename, the
class `gcal callers` and `gcal check` exist for. The GCAL lane passed these two
tasks 6 times of 6 here and 10 of 10 in the rounds before.

## The fix made between the repetitions

After f1, `gcal check` was given a rule for the failure above (commit
`6062619`): in a Java package that is non-null by default (`@NonNullApi`,
`@NullMarked` or `@NonNullByDefault` in its `package-info.java`), a `return`
of `null`, of a `@Nullable` parameter or of a `@Nullable` local, from a method
the change wrote and whose return type has no `@Nullable`, is reported with
its line. A name the method tests against null, passes to an assertion or
assigns is left alone.

Verified without a model:

- On the recorded patch of the failed run it prints
  `AbstractBindingResult.java:415  getRejectedValue returns rejectedValue, which is @Nullable; its return type has no @Nullable`.
- Read as if every line were new, the 4,116 main files of the 366 Spring
  directories that are non-null by default give 4 reports. All four are a
  `return null` without `@Nullable`, in packages Spring's own build excludes
  from NullAway. The scan takes 915 ms.
- 9 unit tests; the agent's checks pass with 239 tests.

What the fix did in agent runs is **not measured**. Both GCAL runs of the
Spring redaction feature made after it passed, and in both `gcal check`
printed only "changed files parse": the agents had written `@Nullable` on
their own. The rule was never shown to an agent that needed it.

## The final check, replayed on all 54 recorded patches

`replay-check.mjs` applies each recorded patch of the nine tasks that change
code (both lanes, three repetitions) to a clone at the task's base commit and
runs the final `gcal check` on it:

```
replayed 54 runs: 3 failed their grader, 3 of those reported; 51 passed, 0 of those reported
```

Each of the three reports names the line the grader failed on: the nullness
line above, `transport.ts:6` as an import of a deleted file with its 19 uses,
and `base_capability_probe.py:66` as a caller of a function that is no longer
declared. The two plain-shell runs did not have this check; whether an agent
that is shown such a report then fixes it is still measured on no run.

## Where GCAL is not ahead

- **StringUtils feature**: more input in 3 of 3 pairs (92k against 87k), the
  same 2 calls, 45 s against 39 s. Unchanged since the goal's summary: the
  task is two calls in either lane and the GCAL lane has the longer prompt.
- **goldeneye rename**: more input in 3 of 3 pairs (112k against 90k) and one
  call more in each (3, 3, 4 against 2, 2, 3); wall time equal.
- **LOD signature refactor**: 342k against 337k at the median, with more calls
  (7 against 5); half the wall time (134 s against 265 s).
- **PyFinance bug**: fewer tokens (0.71) but 82 s against 78 s.
- **Time including preparation**: the whole harness invocation takes the same
  in both lanes (7,201 s against 7,161 s for 35 runs each). Agent and grader
  together take 4,929 s against 6,526 s (0.76), and the harness's own
  maintenance 687 s against 506 s. The rest, 1,585 s against 129 s, is in no
  field of the reports. It is not paid per run: it is the harness building a
  task's index again after the GCAL build changed (`snapshot_refreshed` in
  the report), which happened before 23 of the 35 GCAL runs, because the
  repetitions followed two builds and each task keeps its own snapshot. Those
  23 runs account for 1,518 s (10 to 152 s each); the 12 GCAL runs that found
  their snapshot ready account for 67 s (3 to 13 s each), against 3.7 s per
  plain-shell run. Over those 12 pairs the whole invocation takes 1,946 s
  against 2,362 s (0.82). What the rebuild consists of, and what was done
  about it, is in `2026-11-13-gcal-index-time.md`.
- **Spread**: the agentsview question took 933k and 15 calls in f3 after 381k
  and 337k with 6 calls; the cbm document 915k and 429k. In the 933k run the
  agent asked in fifteen commands with answers of 2 to 32 kB what the run of
  f2 asked in six with answers of 11 to 51 kB; no command failed. It stayed
  below the plain-shell run of its pair (1,208k).

## Not verified

- The cbm document on build `6062619`, and a third repetition of it.
- The effect of the nullness rule on an agent (above).
- A value that is nullable only through what it was assigned from
  (`Object value = ex.getValue()` passed where non-null is required, the
  failure of round 5) is still not seen by `gcal check`.
- Fairness note, as before: the GCAL lane may run `gcal check` after editing;
  plain shell has no equivalent under the one-shot rule.
- One model, one reasoning level, one harness. Nothing here says how another
  agent uses the same commands.

## Budget

| Part | Runs | GCAL | Plain shell | Cumulative |
|---|---|---|---|---|
| f1 | 24 | 3,340k | 6,495k | 9,835k |
| f2, 11 tasks | 22 | 2,202k | 4,866k | 16,903k |
| f2, Spring redaction feature | 2 | 677k | 1,769k | 19,349k |
| f3, 11 tasks | 22 | 2,743k | 4,296k | 26,388k |

Input and output tokens together. 26,388k of about 27M.
