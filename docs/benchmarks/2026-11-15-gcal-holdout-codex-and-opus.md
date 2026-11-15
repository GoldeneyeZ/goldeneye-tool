# Hold-out tasks, with two agents: Codex and Claude Opus 5.5

Date: 2026-11-15. The twelve tasks measured so far are also the tasks GCAL
was tuned on: every fix came from their traces. This note measures five new
tasks that no fix was derived from, and runs them with a second agent.

The measures are the agent's steps (tool calls) and its input and output
tokens. Wall time is an indicator only.

## What was run

- **Five new tasks**, each a kind of work the twelve do not contain, on five
  of the same repositories. Only nine repositories are approved for agent
  runs, so the hold-out is by task and kind, not by repository. The tasks,
  their graders and reference solutions were written by five separate
  authoring agents that were told to design the task as a maintainer would
  and not to look at how GCAL works. I then ran every grader myself on a
  fresh worktree: each fails on the base and passes with its reference.

  | Task | Repository | Kind |
  |---|---|---|
  | `pyfinance-api-change` | PyFinance (Python, about 50 files) | interface change of `validate_weights`, 9 call sites in 3 files |
  | `lod-regression-bug-dirty` | LOD (Python) | a regression hidden in uncommitted work, given a failing test's output; the fault is four files from the failure |
  | `terax-feature-removal` | terax (TypeScript, 451 files) | remove the editor's Vim mode and all that only it used, 10 paths |
  | `vivwallet-config-change` | ViV-Wallet (Java) | two hard-coded labels made configuration, 5 paths |
  | `spring-impact-qa` | Spring Framework (Java, 3 modules) | written impact analysis of a contract change, 14 facts |

- **Two agents**, the same prompt for both:
  - Codex CLI 0.159.0, `gpt-5.6-terra`, reasoning high, as in all earlier runs.
  - Claude Code 2.1.325 in print mode, `claude-opus-5-5`, effort high. No
    user or project customization, no MCP server, no sub-agent; tools `Bash`,
    `Read`, `Edit`, `Write`, `Glob`, `Grep`. New in the harness
    (`--agent claude`, `tools/agent-bench/claude-agent.mjs`).
- **Two lanes**: GCAL (hybrid, with the daemon) and plain shell.
- One run per task, agent and lane: 20 runs, 4,821k tokens (Codex 2,887k,
  Opus 1,933k; the ten Opus runs cost $3.35 at list price as the CLI reports
  it). GCAL build: `3d64c20`, backend binary of `690fc40`.

How the two agents are counted the same way: a shell command is one tool
call for both; for Claude a `Read`, `Grep` or `Glob` is one tool call too,
and an `Edit` or `Write` is a patch, like a Codex patch, which is not a tool
call. Input is everything sent, cached or not. "Not cached" is, for Claude,
the input that was not read from the cache (new input plus what was written
to the cache). Claude sends several tool calls in one model call, so its
model calls are given as well; Codex does not report them.

## Result

All 20 runs pass. One of them (Codex, plain shell, terax) was first failed
by the grader for a reason that is a fault of the grader, described below;
it passes the corrected grader, as do the other three terax runs, the
reference and (still failing) the base.

GCAL / plain shell, and their ratio:

| Task | Codex: calls | Codex: input | Codex: not cached | Opus: calls | Opus: model calls | Opus: input | Opus: not cached | Opus: cost |
|---|---|---|---|---|---|---|---|---|
| PyFinance interface change | 3 / 4, 0.75 | 137k / 203k, 0.68 | 34k / 38k | 3 / 10, 0.30 | 4 / 9 | 54k / 140k, 0.39 | 11k / 14k | $0.16 / $0.21 |
| LOD regression hunt | 3 / 4, 0.75 | 111k / 171k, 0.65 | 36k / 37k | 2 / 1, 2.00 | 3 / 3 | 45k / 41k, 1.10 | 11k / 10k | $0.13 / $0.10 |
| terax feature removal | 12 / 7, 1.71 | 537k / 405k, 1.32 | 33k / 67k | 10 / 15, 0.67 | 11 / 14 | 341k / 446k, 0.77 | 44k / 38k | $0.58 / $0.54 |
| ViV-Wallet config change | 5 / 7, 0.71 | 311k / 462k, 0.67 | 47k / 51k | 6 / 6, 1.00 | 8 / 10 | 265k / 275k, 0.96 | 39k / 30k | $0.52 / $0.45 |
| Spring impact analysis | 4 / 9, 0.44 | 163k / 346k, 0.47 | 38k / 55k | 6 / 16, 0.38 | 7 / 7 | 142k / 130k, 1.09 | 23k / 19k | $0.34 / $0.32 |
| **Sum** | **27 / 31, 0.87** | **1,259k / 1,586k, 0.79** | **188k / 248k, 0.76** | **27 / 48, 0.56** | **33 / 43, 0.77** | **846k / 1,032k, 0.82** | **128k / 111k, 1.16** | **$1.73 / $1.62, 1.07** |

Output tokens are the same in both lanes for both agents (Codex 20.5k /
21.3k, Opus 27.9k / 27.6k).

Read:

- **With Codex, GCAL is ahead on four of the five tasks**, by a quarter to a
  half on calls and on input, and behind on one (terax feature removal: 12
  calls against 7, input 1.32). In sum: calls 0.87, input 0.79.
- **The gain is smaller than on the tasks GCAL was tuned on.** There the sums
  of the medians are 59 calls against 94 (0.63) and 3,099k input tokens
  against 6,061k (0.51) (`2026-11-13-gcal-rename-and-move-steps.md`). Part of
  the difference is the mix: three of the twelve tasks are large ones where
  plain shell spends over a million tokens, and none of the five is. Part is
  what a hold-out is for: these tasks were not tuned on.
- **With Opus, GCAL saves steps and input tokens but not money.** Tool calls
  0.56, model calls 0.77, input 0.82. The input that is not cached is higher
  with GCAL (1.16) and the cost is 7% higher. One clear win (PyFinance: 3
  calls and 54k against 10 and 140k), one on input (terax), and three tasks
  where GCAL gains nothing or costs slightly more.
- One run per cell. A difference of one call or of a few percent on one task
  is within what repeated runs of one build differ by (LOD earlier: 5 to 8
  calls). The sums are more stable than the rows; neither is a median.

## What the traces show

- **PyFinance, both agents: `gcal callers validate_weights` first** (1.8 kB:
  the declaration, the 9 calls, the import lines), then the edit. Opus in
  plain shell reads three files and three ranges one by one (10 calls, 9
  model calls); with GCAL it has all places after one call (3 calls, 4 model
  calls). This is the case the `callers` change of 2026-11-13 was made for,
  on a task it was not made from.
- **terax feature removal, Codex: GCAL reads in many small steps.** After one
  `gcal grep 'Vim|vim' -C 2` (9.4 kB) and one batch of ranges (28.7 kB) the
  run makes eight further calls of 0.2 to 5 kB each, following the close
  callback from layer to layer (`callers EditorPane`, `callers EditorStack`,
  a range, a `grep` in one file, ...). The plain-shell run reads the files
  whole in three calls of 32 to 39 kB and is done in 7. GCAL returned less
  (57 kB against 135 kB) and still used more input (537k against 405k),
  because every extra step sends the whole conversation again. Nothing in
  GCAL answers "everything that exists only for this feature" in one call,
  and the guidance to read ranges instead of whole files works against it
  here.
- **LOD regression hunt: the right tool was `git diff`, and both agents knew
  it.** Opus solves it with one shell call in both lanes (the diff of the
  uncommitted work, the new helper and the test); the GCAL lane then adds the
  `gcal check` the prompt asks for, which is the whole difference (2 calls
  against 1). Codex uses `gcal get ... --tests` and a batch `get` in the GCAL
  lane (3 calls) and `git diff` with whole-file reads in the other (4).
- **Spring impact analysis: Codex and Opus differ.** Codex with GCAL: four
  calls, the first `gcal callers` of the interface method (8.3 kB), against
  nine with `rg` and ranges. Opus in plain shell sends 16 small `Grep` and
  `Read` calls in 7 model calls; with GCAL, 6 calls, also in 7 model calls,
  and 9% more input. For an agent that reads in parallel batches the number
  of tool calls says little; the model calls are equal.
- **The GCAL prompt is 4,055 bytes longer than the plain-shell prompt**
  (6,996 against 2,941 on PyFinance), on every model call. With Codex the
  saved steps outweigh it. With Opus, which takes few model calls in either
  lane, it shows: the input not cached is higher in four of five tasks.

## The grader fault, and what was done

`terax-feature-removal` first failed the Codex plain-shell run with
"`EditorSection.tsx` lost settings rows or labels: Format the file on
explicit save (Cmd+S / :w) ...". The run had changed that description, of
another setting, to "(Cmd+S)": `:w` is one of the Vim commands the task
removes, so the edit is a correct consequence of the task, and the three
other runs and the reference simply left the label. The grader compared the
labels of the settings page verbatim. It now compares them without a Vim
command beside the shortcut; a removed row still fails. Re-run on fresh
worktrees: base fails, reference passes, the four recorded patches pass.
This is a change to a hold-out grader made after seeing a result; it is
recorded here for that reason. Without it the count is 19 of 20, the failure
being in the plain-shell lane.

## Limits

- One run per task, agent and lane; no medians.
- Five tasks, on repositories GCAL has been run on before. A hold-out by
  repository is not possible with the nine approved ones.
- Three of the five graders judge structure only or mostly (ViV-Wallet
  cannot be compiled offline; Spring is a checklist of facts with a pass
  mark of 11 of 14). A pass is not proof of a correct change.
- The Claude driver translates Claude Code's events to the events of Codex.
  It is covered by unit tests and by a check on a real run that the token
  totals equal the sum over the model calls; the cost is the CLI's own
  figure, at list price, on a subscription login.
- The Opus runs were made on the first day the driver existed. Nothing in
  GCAL, its skill or the prompt was ever adjusted to Claude.

## What it points to

1. **The length of the GCAL prompt.** It is the one cost GCAL adds to every
   model call, it has grown with each fix, and with an agent that needs few
   calls it cancels the gain.
2. **Reading for a removal or a trace through layers.** One call that
   returns, for a symbol or a word, the enclosing declarations whole (not
   ranges asked one at a time) would replace the nine small calls of the
   terax run.
3. **`gcal check` as an obligation.** On a task solved by one command it is
   half of the calls. The prompt could ask for it only after edits in more
   than one file.

What was done about the three, and one more run of these tasks:
`2026-11-15-gcal-short-prompt-holdout-rerun.md`.
