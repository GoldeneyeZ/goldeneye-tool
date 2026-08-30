# Abyssal Zenith benchmark: GCAL impact over Goldeneye

Run date: 2026-07-22

## Question

What changes when agents use GCAL as the sole model-facing wrapper over Goldeneye instead of calling Goldeneye MCP tools directly?

## Controlled setup

- Repository: `C:\Users\Zacha\IdeaProjects\abyssal-zenith`
- Commit: `92060daeb601f397020e665722bc5cfefd7f44a4`
- Task: centralize Spring Security public endpoint matching and add focused tests
- Model: `gpt-5.6-terra`, high reasoning
- Repetitions: three cold and three warm GCAL runs
- Grader: held-out JUnit test plus focused Maven invocation
- Source policy: GCAL-only for Java discovery/source reads; raw Java reads and direct Goldeneye MCP calls prohibited
- GCAL backend: `target/release/goldeneye.exe` over stdio
- Isolation per lane: `GCAL_HOME`, `GOLDENEYE_DB_PATH`; `GCAL_MCP_URL` and `GCAL_PROJECT` unset
- Attribution assertion: separate `CBM_CACHE_DIR` decoy remained empty in every run

Cold GCAL agents ran `gcal init` inside their turn. Warm GCAL lanes ran `gcal init <worktree>` before the agent and recorded it as preindex time.

## Artifacts

- GCAL-only raw report: `target/agent-bench/abyssal-zenith-goldeneye-code-agent-layer.json`
- Prior raw report: `target/agent-bench/abyssal-zenith-goldeneye-serena-vanilla.json`
- Merged normalized report: `target/agent-bench/abyssal-zenith-goldeneye-code-agent-layer-serena-vanilla-merged.json`
- GCAL config: `tools/agent-bench/abyssal-zenith-goldeneye-code-agent-layer.config.json`

The merged report preserves each input's `raw_success` and recomputes `success` consistently from agent exit, timeout, grader, and current protocol detection. This corrects two harness classifications:

- one prior direct-Goldeneye cold run passed the grader but the old predicate rejected recovered index-call failures;
- one GCAL cold run passed the grader but `.xml` was missing from the non-source search allowlist, so `rg "...junit" pom.xml` was incorrectly classified as a Java-source read.

## Results

Medians use normalized successful runs. P95 is interpolated over only two or three successful samples and is directional.

| Lane | Success | Agent wall p50 / p95 | Preindex p50 | Verified E2E p50 | Total tokens p50 | Uncached input p50 | Output p50 | Tools p50 | Discovery calls p50 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Vanilla | 3/3 | 92.2s / 96.2s | 0.0s | 102.3s | 247,652 | 23,154 | 3,126 | 6 | 0 |
| Goldeneye direct cold | 3/3 | 126.4s / 173.9s | 0.0s | 136.5s | 652,351 | 50,165 | 4,185 | 14 | 9 MCP |
| Goldeneye direct warm | 2/3 | 103.6s / 106.3s | 3.5s | 117.0s | 476,470 | 45,555 | 3,524 | 12 | 9.5 MCP |
| Goldeneye + GCAL cold | 3/3 | 140.5s / 147.3s | 0.0s | 160.7s | 370,104 | 32,708 | 4,186 | 12 | 8 GCAL |
| Goldeneye + GCAL warm | 3/3 | 135.7s / 139.9s | 5.0s | 160.8s | 330,186 | 17,074 | 3,783 | 10 | 7 GCAL |
| Serena cold | 3/3 | 252.1s / 280.5s | 0.0s | 264.6s | 827,267 | 55,307 | 4,830 | 18 | 15 MCP |
| Serena warm | 3/3 | 142.6s / 168.4s | 125.4s | 283.2s | 510,102 | 30,276 | 4,443 | 24 | 19 MCP |

### GCAL versus direct Goldeneye

| Comparison | Agent wall | Verified E2E | Total tokens | Uncached input | Tool calls |
|---|---:|---:|---:|---:|---:|
| GCAL cold vs direct cold | +11.1% | +17.7% | **−43.3%** | **−34.8%** | −14.3% |
| GCAL warm vs direct warm | +30.9% | +37.5% | **−30.7%** | **−62.5%** | −16.7% |

Observed discovery-result bytes are not identical wire formats: direct MCP uses JSON result envelopes; GCAL uses captured stdout. They still show the payload trend:

| Lane | Observed discovery-result bytes p50 |
|---|---:|
| Goldeneye direct cold | 42,623 |
| Goldeneye direct warm | 68,536 |
| Goldeneye + GCAL cold | 8,090 |
| Goldeneye + GCAL warm | 8,218 |

GCAL reduced observed result payload by about 81% cold and 88% warm. Response formatting/filtering is working.

### GCAL versus vanilla

- GCAL cold: wall +52.3%, verified E2E +57.1%, total tokens +49.4%.
- GCAL warm: wall +47.1%, verified E2E +57.2%, total tokens +33.3%.
- GCAL warm uncached input was 26.3% lower than vanilla (`17,074` vs `23,154`) despite 33.3% higher total tokens.

That last result is the strongest workflow signal: GCAL responses are compact enough to reduce unique input below vanilla, but repeated command turns replay more cached context and increase total tokens/time.

### GCAL versus Serena

- GCAL cold: wall −44.3%, verified E2E −39.3%, total tokens −55.3%.
- GCAL warm: wall −4.9%, verified E2E −43.2%, total tokens −35.3%.
- GCAL/Goleneye cache: about 36 MiB; Serena: about 622 MiB.

## GCAL command behavior

Across six runs:

- 46 GCAL commands: `init` 3, `search` 20, `get` 15, `status` 5, `symbol` 2, `--help` 1
- 8 failures: 7 `search`, 1 transient `get`
- median: one failed GCAL command per run
- median observed GCAL stdout: about 8.2 KiB/run
- Goldeneye database: about 18.8 MiB/run
- CBM decoy files: zero in every run
- held-out grader: 6/6

Failure classes:

1. Four agents used `gcal search "SecurityConfig|JwtAuthenticationFilter"`; raw FTS rejected `|`.
2. One used `gcal search "*Test"`; raw FTS rejected leading `*`.
3. One chained three searches; final empty `@SpringBootTest` search made the shell command fail despite useful earlier output.
4. One guessed unsupported `gcal search --kind`; current option is `--label`.
5. One `gcal get` failed once with Windows `Io` code 267 (`NotADirectory`) and succeeded immediately with the identical retry.

## Optimization ownership

### Goldeneye graph layer

1. Return typed query errors instead of leaking `SQLite store error: fts5 ...`.
2. Define literal-safe search semantics or a clear typed distinction between literal, FTS, and regex modes.
3. Improve startup/database-open latency if GCAL continues spawning one Goldeneye stdio process per command.
4. Keep intrinsic symbol relevance and candidate ranking inside Goldeneye.
5. Investigate whether duplicated Java class/constructor naming increases selection work.

### GCAL response/workflow layer

1. Escape or normalize model-supplied search text before calling Goldeneye; reserve `gcal symbol` for regex.
2. Consider `--kind` as an alias for `--label`, or expose clearer command-schema/help hints.
3. Add a bounded multi-symbol/context command to retrieve two known classes in one round trip. Do not depend on `gcal elect` in Phase 1.
4. Avoid shell-chain ambiguity; one GCAL invocation should return structured per-query status when batching.
5. Add structured telemetry: backend identity, backend startup, graph query time, GCAL formatting time, result rows/bytes.
6. Diagnose transient Windows `NotADirectory` from `gcal get`.
7. Reduce process launches and agent turns. This now matters more than further output compaction.

## Main conclusion

GCAL successfully transforms Goldeneye's rich MCP responses into much smaller agent-facing output and cuts total tokens by 31–43% versus direct Goldeneye. It also achieved 6/6 grader correctness in this sample.

GCAL currently trades those savings for 11–31% longer agent wall time and 18–38% longer verified E2E time versus direct Goldeneye. Warm GCAL's unique input is already below vanilla, so next improvement should focus on fewer round trips, safer search syntax, lower process/startup overhead, and zero failed commands—not more aggressive response truncation.
