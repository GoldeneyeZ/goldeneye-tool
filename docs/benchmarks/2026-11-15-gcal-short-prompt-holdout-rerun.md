# A shorter prompt and a fuller `callers`: the hold-out tasks again

Date: 2026-11-15. The hold-out round of the same day
(`2026-11-15-gcal-holdout-codex-and-opus.md`) pointed at three costs: the
length of the GCAL prompt, reading through layers in many small calls, and
`gcal check` as an obligation. This note records what was changed for each
and one more run of the five hold-out tasks in the GCAL lane, with both
agents.

The measures are the agent's steps (tool calls) and its input and output
tokens. Wall time is an indicator only.

## What was changed (commit `276a73b`)

- **The prompt.** The skill went from 10,356 to 2,766 bytes; the workflow
  API, project state and command details moved to `reference.md` beside it.
  The GCAL lane of the benchmark prompt went from 6,996 to 4,127 bytes on
  PyFinance: 1,186 bytes more than the plain-shell prompt, where it was
  4,055 more. One rule moved from the prompt into a command: `gcal replace`
  given one bare word now names the changed lines that had the word only in
  a comment or a string.
- **`gcal callers`**, from the trace of the terax feature removal, where
  Codex spent five calls on a prop passed down three components:
  a declaration, a call or a JSX element that goes on over the next lines is
  printed to its end; `<Name` counts as a use of a component (a function
  component had "0 callers"); a name that two or three callables share is
  answered for all of them instead of `# ambiguous`. On a copy of terax,
  `gcal callers EditorStack` now prints in one answer what calls 4, 6, 7 and
  8 of that trace asked for.
- **`gcal check`** is asked for after edits in more than one file, no longer
  after every change.

No new option that prints enclosing declarations whole was added, though the
earlier note proposed one: the trace showed the calls were spent on the
`callers` chain, and that was fixed instead.

## What was run

The five hold-out tasks, GCAL lane only, one run each with Codex
(`gpt-5.6-terra`, reasoning high) and Claude Code (`claude-opus-5-5`, effort
high): 10 runs, 2,197k tokens (Codex 1,429k, Opus 768k, $1.61). The
plain-shell lane was not run again; its figures are those of the first
round, made earlier the same day.

## Result

All 10 runs pass.

GCAL first round / GCAL this round / plain shell:

| Task | Codex: calls | Codex: input | Codex: not cached | Opus: calls | Opus: model calls | Opus: input | Opus: not cached | Opus: cost |
|---|---|---|---|---|---|---|---|---|
| PyFinance interface change | 3 / 5 / 4 | 137k / 178k / 203k | 34k / 32k / 38k | 3 / 3 / 10 | 4 / 4 / 9 | 54k / 49k / 140k | 11k / 15k / 14k | $0.16 / $0.18 / $0.21 |
| LOD regression hunt | 3 / 6 / 4 | 111k / 208k / 171k | 36k / 34k / 37k | 2 / 1 / 1 | 3 / 3 / 3 | 45k / 42k / 41k | 11k / 10k / 10k | $0.13 / $0.11 / $0.10 |
| terax feature removal | 12 / 11 / 7 | 537k / 445k / 405k | 33k / 37k / 67k | 10 / 10 / 15 | 11 / 11 / 14 | 341k / 295k / 446k | 44k / 36k / 38k | $0.58 / $0.48 / $0.54 |
| ViV-Wallet config change | 5 / 8 / 7 | 311k / 356k / 462k | 47k / 50k / 51k | 6 / 6 / 6 | 8 / 8 / 10 | 265k / 243k / 275k | 39k / 38k / 30k | $0.52 / $0.52 / $0.45 |
| Spring impact analysis | 4 / 6 / 9 | 163k / 222k / 346k | 38k / 26k / 55k | 6 / 5 / 16 | 7 / 6 / 7 | 142k / 113k / 130k | 23k / 21k / 19k | $0.34 / $0.33 / $0.32 |
| **Sum** | **27 / 36 / 31** | **1,259k / 1,410k / 1,586k** | **188k / 179k / 248k** | **27 / 25 / 48** | **33 / 32 / 43** | **846k / 742k / 1,032k** | **128k / 119k / 111k** | **$1.73 / $1.61 / $1.62** |

GCAL against plain shell, first round and this round:

| | Codex: calls | Codex: input | Codex: not cached | Opus: calls | Opus: model calls | Opus: input | Opus: not cached | Opus: cost |
|---|---|---|---|---|---|---|---|---|
| First round | 0.87 | 0.79 | 0.76 | 0.56 | 0.77 | 0.82 | 1.16 | 1.07 |
| This round | 1.16 | 0.89 | 0.72 | 0.52 | 0.74 | 0.72 | 1.08 | 0.99 |

Output tokens are unchanged (Codex 19.3k, Opus 26.5k).

Three marks were set before the run:

- **Opus, cost no higher than plain shell: met**, at $1.61 against $1.62.
  That is one cent on one run per task, so it reads as "equal", not as
  "cheaper". Input went from 0.82 to 0.72 of plain shell and the input not
  cached from 1.16 to 1.08; every Opus figure moved the right way.
- **Codex, terax in 8 calls or fewer: not met**, 11 (12 before, 7 in plain
  shell).
- **No pass lost: met.**

And one result that was not expected:

- **Codex made more calls on four of the five tasks**: 36 against 27, which
  is also more than plain shell (31). Input is still below plain shell
  (0.89) but above the first round (0.79).

One run per cell. Repeated runs of one build have differed by three calls on
one task before (LOD refactor: 5 to 8), so a single row proves little; four
rows moving the same way by two or three calls each is more than I would put
down to chance, and less than a measurement.

## What the traces show

**Opus.** The runs make as many model calls as in the first round (4, 3, 11
and 8 on four tasks, 6 for 7 on Spring) and send less with each. LOD goes
from 2 tool calls to 1 because `gcal check` is no longer asked for after a
one-file fix.

**Codex.** `gcal check` was run in the three tasks that edit several files
and in neither of the others, as the new wording asks. The extra calls are
elsewhere:

- *PyFinance* (5 for 3): `gcal callers` first, as before. Then
  `gcal get validate_weights run_weight_backtest --tests` (13.9 kB), which
  prints four of the eight tests of `validate_weights` whole and lists the
  other four as rows; two more `get` calls fetch those four and
  `ExecutionConfig`. In the first round the
  run named the two test files in that `get` and had everything in one
  answer of 31 kB.
- *LOD* (6 for 3): the uncommitted diff read in three separate calls and a
  file with `Get-Content`, where the first round chained the diffs behind a
  `gcal get` in two calls.
- *ViV-Wallet* (8 for 5): begins with `gcal search`, reads the same four
  files twice (`gcal get`, 22 kB, then `Get-Content`, 26 kB), and after the
  edit asks `gcal callers` twice "checking all constructor call sites"
  before `gcal check`.
- *Spring* (6 for 4): six calls of 4 to 18 kB where the first round made
  four.
- *terax* (11 for 12): the `callers` changes were used once.
  `gcal callers src/modules/editor/EditorStack.tsx:16` printed the
  `<EditorStack ... />` element of `WorkspaceSurface.tsx` with its six props,
  a use the first round's `callers` did not list at all. The run did not ask
  `gcal callers EditorPane`, and it met two other gaps on the way:
  `gcal callers performSaveRef onCloseRef` answered
  `# not found: performSaveRef` (a local ref is not a symbol, and the second
  name was dropped without a word), and `gcal get EditorStack --tests`
  answered `# ambiguous` (210 bytes), each followed by a call that asked
  again another way.

Three of these (a search for a name the task gives, reading through both
surfaces, discovery after the edit) are what three sentences of the old
prompt spoke against: "Name gcal targets directly instead of searching for
identifiers", "never ask the same question through both" (kept), "Stop
discovery once enough evidence exists". Two of the three were cut. That is a
reading of five traces, not a test: the same runs could differ this much
with the old prompt.

## What was changed after the run (not measured)

- One sentence went back into the GCAL prompt, about 190 bytes: name targets
  directly, edit from what an answer printed without reading it again, stop
  discovery once there is enough evidence. The skill never lost these.
- `gcal callers` takes several names and answers each. A name no symbol has
  (a local, a ref) is answered with its text matches instead of
  `# not found`.
- `gcal get` on a name two or three symbols share prints them all when they
  are together at most 240 lines (before: only when each was at most 40).

On a copy of terax, `gcal callers performSaveRef onCloseRef` now lists the
places that use the two refs, and `gcal get EditorStack` prints both
functions.

## Limits

- One run per task and agent. No medians, and the plain-shell lane is from
  the first round.
- The five tasks are no longer unseen: the `callers` changes came from the
  trace of one of them, and the changes made after this run from the traces
  of all five. A further run on them measures a fit.
- The twelve tasks GCAL was tuned on were not run with the short prompt. The
  rules that were cut came from their traces; whether cutting them costs
  steps there is not known.
- The Opus cost is the CLI's own figure, at list price, on a subscription
  login.

## What it points to

1. **For Opus the short prompt is a clear gain and GCAL now costs the same
   as plain shell** on these tasks, with half the tool calls and
   three-quarters of the model calls and of the input.
2. **For Codex the short prompt is unproven.** The honest state is: input
   0.89 of plain shell on this round, 0.79 on the round before, and more
   calls than plain shell on this one. One more Codex round in the GCAL lane
   (about 1.4M tokens) on the build with the restored sentence would say
   whether the calls come back down; two rounds would say how much of the
   difference is chance.
3. **Each trace still turns up a small gap of the tool** (a second name
   dropped, a local name refused). These are found faster by using GCAL on
   real work than by further rounds on seventeen tasks that have all been
   seen.
