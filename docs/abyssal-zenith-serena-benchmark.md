# Abyssal Zenith agent benchmark: Goldeneye vs Serena vs vanilla

Run date: 2026-07-22

## Outcome

Goldeneye outperformed Serena on the Abyssal Zenith Java/Spring task in both cold and warm modes. It did not outperform the vanilla agent: vanilla remained faster, more token-efficient, and more reliable in this 15-run sample.

The most actionable result is Serena's warm-up cost. Its median explicit LSP pre-index was 125.4 seconds and its median cache was 621.9 MiB. Goldeneye's corresponding medians were 3.5 seconds and 36.0 MiB. Including setup, indexing, execution, and grading, Goldeneye warm completed in a median 117.0 seconds versus Serena's 283.2 seconds.

## Reproducible setup

- Target repository: `C:\Users\Zacha\IdeaProjects\abyssal-zenith`
- Pinned commit: `92060daeb601f397020e665722bc5cfefd7f44a4`
- Project: Java 21, Spring, Maven
- Model: `gpt-5.6-terra`, high reasoning
- Runs: three cold and three warm per graph engine, plus three vanilla runs; 15 randomized runs total
- Serena: `Serena 1.6.2.dev0`, isolated `SERENA_HOME` per run, `--context=codex --project <worktree>`
- Goldeneye: release binary, isolated graph state per run
- Correctness: deterministic structural checks followed by an injected held-out JUnit 5 test and a focused Maven test run
- Source-access policy: Goldeneye and Serena agents could not directly read Java source with shell or built-in file-reading tools. They had to use their assigned discovery server. Vanilla was unrestricted.
- Editing policy: all lanes used the normal editing surface; Goldeneye and Serena were discovery/read tools, not source-writing tools.

The task centralized duplicated public Spring Security endpoint matching into a `PublicEndpointPaths` utility, wired both `SecurityConfig` and `JwtAuthenticationFilter` to it, and required focused tests. The held-out cases checked exact paths, Ant-style `/**` base and descendant behavior, null handling, defensive copying, and lookalike-prefix rejection.

Run the benchmark from the Goldeneye repository:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/abyssal-zenith-goldeneye-serena-vanilla.config.json `
  --skip-build
```

Remove `--skip-build` after changing Goldeneye source.

## Results

Medians below use only grader-verified successful runs. Wall p95 is linearly interpolated across the small three-run samples and should be treated as directional.

| Lane | Verified | Agent wall p50 / p95 | Pre-index p50 | Completion p50 | Verified E2E p50 | Total tokens p50 | Uncached input p50 | Tool / MCP calls p50 | MCP failures p50 | Cache p50 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Goldeneye cold | 3/3 | 126.4s / 173.9s | 0.0s | 126.4s | 136.5s | 652,351 | 50,165 | 14 / 9 | 2 | 26.0 MiB |
| Goldeneye warm | 2/3 | 103.6s / 106.3s | 3.5s | 107.1s | 117.0s | 476,470 | 45,555 | 12 / 9.5 | 2 | 36.0 MiB |
| Serena cold | 3/3 | 252.1s / 280.5s | 0.0s | 254.6s | 264.6s | 827,267 | 55,307 | 18 / 15 | 0 | 621.8 MiB |
| Serena warm | 3/3 | 142.6s / 168.4s | 125.4s | 273.1s | 283.2s | 510,102 | 30,276 | 24 / 19 | 0 | 621.9 MiB |
| Vanilla | 3/3 | 92.2s / 96.2s | 0.0s | 92.2s | 102.3s | 247,652 | 23,154 | 6 / 0 | 0 | 0.0 MiB |

`Completion` includes engine setup, optional pre-indexing, and agent execution. `Verified E2E` additionally includes grading. Cached-input tokens are included in total token counts; uncached input is shown separately to expose prompt-cache effects.

### Direct comparisons

- Against Serena cold, Goldeneye cold had 49.9% lower median agent wall time, 48.4% lower verified E2E time, and 21.1% fewer total tokens.
- Against Serena warm, Goldeneye warm had 27.3% lower median agent wall time and 58.7% lower verified E2E time. Total tokens were 6.6% lower.
- Goldeneye's median cache footprint was 95.8% smaller cold and 94.2% smaller warm than Serena's.
- Against vanilla, Goldeneye cold took 37.1% longer at agent wall and used 2.63x the total tokens. Goldeneye warm took 12.4% longer at agent wall and used 1.92x the total tokens.
- Vanilla also achieved 3/3 correctness, while Goldeneye warm achieved 2/3. Beating vanilla therefore remains the primary performance and robustness target.

## Correctness and classification note

The raw JSON says Goldeneye cold succeeded 2/3. One additional cold run actually exited cleanly, produced the correct four-file patch, passed the held-out Maven grader, and had no protocol violation. The old in-process predicate marked it failed solely because the agent made two invalid `index_repository` attempts before recovering. The harness now defines task success from agent exit, timeout, grader result, and protocol compliance; MCP and indexing failures remain separately visible telemetry. Under that corrected predicate, Goldeneye cold is 3/3.

There was one genuine failure: `abyssal-public-endpoints-warm-goldeneye-3` made no patch and failed the grader after 36.9 seconds. Failed runs are excluded from timing medians so a fast failure cannot improve a lane's performance score.

Goldeneye's successful runs still had a median of two MCP failures, while Serena had none. These were recoverable API-usage errors, but they are a concrete ACK/tool-guidance optimization opportunity.

## Java indexing prerequisite discovered

The production Goldeneye bootstrap was using `CoreGrammarProvider`, so its first Abyssal Zenith index covered only 4 of 559 discovered files. The benchmark work enables the syntax crate's `full-grammar-pack` feature in `goldeneye-bootstrap` and uses a core-first provider for both indexing and syntax services. Core languages retain their existing built-in grammar identity; Java and other non-core languages fall back to the materialized full pack.

An initial direct switch to `FullGrammarProvider` exposed a compatibility regression in durable Rust edits because the full-pack Rust grammar provenance did not match the built-in core grammar used by that subsystem. The core-first provider resolved it, and the full CLI recovery and workspace suites pass. Java continues through the same full-pack path used during the benchmark, so this correction does not change the target-language behavior measured here.

After that correction, Goldeneye indexed:

- 562 discovered files
- 558 parsed files and 4 reused files
- 325 Java files represented in the architecture summary
- 7,030 nodes and 8,822 edges
- approximately 4.6 seconds for the validation index

Without this correction, a Goldeneye-versus-Serena Java comparison would not have equivalent code coverage.

## Artifacts

- Full raw result: `target/agent-bench/abyssal-zenith-goldeneye-serena-vanilla.json`
- Config: `tools/agent-bench/abyssal-zenith-goldeneye-serena-vanilla.config.json`
- Task: `tools/agent-bench/tasks/abyssal-public-endpoints.md`
- Grader: `tools/agent-bench/graders/abyssal-public-endpoints.mjs`
- Cold smoke: `target/agent-bench/abyssal-zenith-smoke.json`
- Serena warm smoke: `target/agent-bench/abyssal-zenith-serena-warm-smoke.json`
- Goldeneye warm smoke: `target/agent-bench/abyssal-zenith-goldeneye-warm-smoke.json`
- Post-fix Goldeneye cold smoke: `target/agent-bench/abyssal-zenith-goldeneye-core-first-smoke.json`

The runner uses detached temporary worktrees and removed all benchmark worktrees after completion. The source Abyssal Zenith checkout remains at the pinned commit with only its two pre-existing user modifications.

## Verification

- `node --test tools/agent-bench/core.test.mjs`: 7 passed
- Node syntax checks for runner and grader: passed
- Held-out grader against a known-correct temporary implementation: passed
- `cargo fmt --all --check`: passed
- `cargo test -p goldeneye-bootstrap`: 3 passed
- Full-grammar provider contracts: 6 passed
- CLI stdio/recovery integration tests: 5 passed
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
- `git diff --check`: passed; only line-ending conversion warnings were emitted for existing working-tree files
- Release Goldeneye build with the materialized grammar pack: passed
- Post-fix cold Goldeneye smoke: passed in 107.0 seconds with 594,744 total tokens and a passing held-out grader

## Next optimization targets

1. Make ACK/tool instructions prevent invalid `index_repository` argument sequences; recovered failures still cost turns and tokens.
2. Diagnose the one run where the Goldeneye MCP surface was not used and add a deterministic startup/readiness check before agent launch.
3. Improve Goldeneye result ranking and response compactness. Even warm Goldeneye used about 1.92x vanilla's tokens.
4. Add at least two more Java/Spring tasks and increase repetitions before treating the measured gaps as general rather than task-specific.
5. Keep the held-out Maven grader focused. A full Spring test suite would make build time dominate the tool comparison.

## Serena references

- [Serena quick start](https://github.com/oraios/serena#quick-start)
- [Serena Codex integration](https://oraios.github.io/serena/02-usage/030_clients.html#codex)
