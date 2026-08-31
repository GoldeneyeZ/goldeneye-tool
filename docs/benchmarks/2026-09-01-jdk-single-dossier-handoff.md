# JDK single-dossier GCAL benchmark handoff

## Goal

Beat the Vanilla lane on a large OpenJDK `jpackage --app-resources` task by
moving adaptive discovery into one true-JavaScript GCAL workflow. The workflow
must loop over search results, select relevant files, retrieve sources, inspect
callers/callees, and return one bounded implementation dossier.

Repositories:

- GCAL/Goldeneye: `/home/goldeneye/IdeaProjects/goldeneye-tool`
- JDK benchmark seed: `/home/goldeneye/IdeaProjects/jdk-app-resources-bench`
- Original JDK clone: `/home/goldeneye/IdeaProjects/jdk`
- Seed commit: `a4ded467e7193aa160da21eac451694232272e65`

Model/lane settings: `gpt-5.6-terra`, high reasoning, warm Goldeneye cache,
full grammar pack, one-shot policy, no agent-run verification commands.

## Current conclusion

Beating Vanilla is possible on discovery latency and GCAL round trips. It is
not yet demonstrated end-to-end on correctness plus total tool calls.

Best broad implementation run (`r1i`) used one GCAL call and was faster than
Vanilla, but still failed two grader requirements. Free-form workflow programs
remain fragile: output size, invalid source candidates, stale trace targets,
shell quoting, guessed file paths, and regex-vs-glob syntax each caused a
separate failed attempt. Product fixes now cover most of those failure modes.

Next work should stop adding task-specific prompt prose. Build or validate a
canonical dossier program/helper with a small declarative bucket definition,
then benchmark that stable surface.

## Benchmark evidence

| Attempt | Result | Wall | Total tools | GCAL calls | Total tokens | Uncached input | Patch files | Main outcome |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Vanilla final 1 | fail | ~520 s | 36 | 0 | ~4.37 M | ~159 k | 26 | Functional work mostly complete; manual-doc grader failed |
| GCAL `r1e` | fail | 328 s | 9 | 4 | 1.20 M | 74,095 | 6 | Dossier efficient; implementation too narrow |
| GCAL `r1i` | fail | 473 s | 42 | 1 | 4.07 M | 166,290 | 14 | All functional checks passed except manual wording and minimum focused-test coverage |
| GCAL `r1l` | fail | 515 s | 9 | 1 | 0.72 M | 74,724 | 0 | Clean workflow transport; guessed filters left parser/layout/docs buckets empty; agent reverted partial edits |
| GCAL `r1m` | fail | 61 s | 1 | 1 | 0.07 M | 25,050 | 0 | Agent emitted glob file patterns; backend expected regex |
| GCAL `r1n` | interrupted | n/a | n/a | n/a | n/a | n/a | n/a | PC/session crash before artifact creation |

Reports:

- `target/agent-bench/jdk-jpackage-app-resources/one-shot/20260831-jdk-vanilla-final-1/report.json`
- `target/agent-bench/jdk-jpackage-app-resources/one-shot/20260831-jdk-gcal-dossier-r1e/report.json`
- `target/agent-bench/jdk-jpackage-app-resources/one-shot/20260831-jdk-gcal-dossier-r1i/report.json`
- `target/agent-bench/jdk-jpackage-app-resources/one-shot/20260831-jdk-gcal-dossier-r1l/report.json`
- `target/agent-bench/jdk-jpackage-app-resources/one-shot/20260901-jdk-gcal-dossier-r1m/report.json`

`r1i` is the strongest evidence for the hypothesis:

- one model-facing GCAL discovery call;
- 47 seconds faster than Vanilla;
- ~300k fewer total tokens;
- correct parser/model/layout/copy/platform/help implementation;
- missed explicit manual separator/destination/precedence wording;
- changed only two focused Java test files; grader required four;
- total tools remained worse than Vanilla (42 vs 36) because editing was split
  across many file-change operations.

## Shipped implementation

All commits below are on `master` and pushed to `origin/master`:

- `32c40d8` — single-dossier benchmark strategy and protocol telemetry
- `f9b17ec` — bounded dossier workflow fanout
- `6695b2d` — literal-safe dossier search guidance
- `56e4a26` — bounded workflow output guidance
- `1ead206` — balanced evidence-bucket selection
- `ad2dfb1` — `gcal.trySource`
- `2f2653c` — evidence targeting by file
- `b0e5441` — exactly one GCAL discovery invocation
- `9f968ba` — flattened settled source results (`result.source` directly)
- `a49df61` — `gcal.tryCallers` / `gcal.tryCallees`
- `10699cd` — file-based multi-line workflows; avoids inline shell quoting
- `3de581c` — in-workflow weak-bucket retries
- `4dba7c1` — regex-or-glob file-pattern normalization

Current JS workflow API:

- `gcal.search(query, options)`
- `gcal.select(rows, index)`
- `gcal.source(name)` / `gcal.get(name)`
- `gcal.trySource(name)` → `{ ok: true, ...sourceFields }` or
  `{ ok: false, error }`
- `gcal.callers(name, options)` / `gcal.callees(name, options)`
- `gcal.tryCallers(name, options)` / `gcal.tryCallees(name, options)` →
  `{ ok: true, edges }` or `{ ok: false, error }`

Workflow programs should use `--file`, not inline `--js`, when multi-line.
`filePattern` now accepts backend regex plus common `*`/`**` glob spelling.

Validation at `4dba7c1`:

```text
59 targeted Vitest tests passed
pnpm build passed
```

## Active benchmark contract

`tools/agent-bench/bin/benchmark-agent-tasks.mjs` recognizes engine setting:

```json
"gcal_discovery_strategy": "single-dossier"
```

Contract:

1. Agent writes `/tmp/gcal-single-dossier.js`.
2. First and only GCAL command is:
   `gcal workflow --file /tmp/gcal-single-dossier.js`.
3. Workflow uses six evidence buckets and at most 32 backend calls.
4. Empty buckets may retry inside the same workflow.
5. Source/trace misses use settled APIs and cannot reject the whole workflow.
6. Returned dossier stays below 36 KiB.
7. Agent performs no later GCAL call.

The current prompt still asks the model to author the entire dossier program.
That remains the largest reliability risk.

## Resume state

The local temp config was `/tmp/gcal-jdk-app-resources.json`. Before the crash it
pointed at snapshot/worktree/cache suffix `v27` and attempt ID
`20260901-jdk-gcal-dossier-r1n`. The attempt produced no report. Treat all `v27`
paths as orphaned and use a fresh suffix (`v28` or later).

Expected config essentials:

```json
{
  "repo": "/home/goldeneye/IdeaProjects/jdk-app-resources-bench",
  "base_ref": "a4ded467e7193aa160da21eac451694232272e65",
  "model": "gpt-5.6-terra",
  "reasoning": "high",
  "engines": [{
    "id": "goldeneye-code-agent-layer",
    "kind": "gcal",
    "gcal_discovery_strategy": "single-dossier",
    "env": {
      "GCAL_BACKEND": "goldeneye",
      "GOLDENEYE_GRAMMAR_PACK": "full",
      "GOLDENEYE_INCLUDE_PATHS": "src/jdk.jpackage;test/jdk/tools/jpackage"
    }
  }]
}
```

Required launcher:

```sh
mkdir -p /tmp/gcal-bin
printf '%s\n' '#!/bin/sh' \
  'exec /home/goldeneye/.nvm/versions/node/v24.11.1/bin/node /home/goldeneye/IdeaProjects/goldeneye-tool/agent/dist/main.js "$@"' \
  > /tmp/gcal-bin/gcal
chmod +x /tmp/gcal-bin/gcal
```

Benchmark command:

```sh
cd /home/goldeneye/IdeaProjects/goldeneye-tool/tools/agent-bench
export JAVA_HOME=/home/goldeneye/.sdkman/candidates/java/26.0.2+1.1-tem
export PATH="$JAVA_HOME/bin:/home/goldeneye/.nvm/versions/node/v24.11.1/bin:/home/goldeneye/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
node ./bin/benchmark-agent-tasks.mjs \
  --config /tmp/gcal-jdk-app-resources.json \
  --one-shot \
  --task jdk-jpackage-app-resources \
  --engine goldeneye-code-agent-layer \
  --cache-modes warm \
  --repetitions 1 \
  --model gpt-5.6-terra \
  --reasoning high \
  --skip-build \
  --attempt-id 20260901-jdk-gcal-dossier-r1n-retry
```

## Recommended next steps

1. Confirm `master` clean and `agent/dist` rebuilt from `4dba7c1`.
2. Remove or ignore orphaned `v27` benchmark worktree/cache; create fresh `v28`.
3. Add an integration test proving glob normalization reaches the real
   Goldeneye backend for `src/jdk.jpackage/**/*.java` and `**/*.properties`.
4. Replace free-form dossier generation with a checked-in generic template or
   a first-class helper. Model should provide only bucket query/pattern config.
5. Re-run one GCAL attempt. Require: one successful workflow, no protocol
   violations, all six buckets populated, grader pass.
6. Only after correctness: run three GCAL and three Vanilla repetitions.
7. Optimize edit batching if GCAL remains above Vanilla total tool calls.

Do not claim a win from failed runs. Required comparison is successful-run
correctness, wall time, verified end-to-end time, total/uncached tokens, total
tool calls, GCAL calls, and variance over repeated samples.

## Disk cleanup

On 2026-08-31, `/home/goldeneye/IdeaProjects/goldeneye-tool/target/debug`
(15 GiB) was moved to desktop Trash using `gio trash`. It is recoverable until
Trash is emptied. `target/release` and benchmark artifacts were preserved.
