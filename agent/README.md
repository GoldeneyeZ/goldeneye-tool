# Goldeneye Code Agent Layer

Goldeneye Code Agent Layer (GCAL) is a local context-election CLI and workflow kit for coding agents. It helps agents discover code symbols, inspect metadata, trace relationships, check architecture, and fetch exact source without flooding the conversation context.

GCAL Phase 1 uses Goldeneye as its primary code-graph backend and exposes deterministic commands for compact, repeatable codebase lookup.

## Install And Build

On Windows, run the installer from the Goldeneye repository root to resolve Goldeneye, build GCAL, install the `gcal` command globally, configure Goldeneye as the default backend, and install the Codex workflow skill:

```powershell
.\agent\install.ps1
```

The installer copies the GCAL skill assets to `$HOME\.codex\skills\goldeneye-code-agent-layer` and updates `$HOME\.codex\AGENTS.md` inside a managed GCAL block. It also removes recognized legacy GCAL assets and stale backend configuration. GCAL remains the sole model-facing code-discovery surface while available. Verify the CLI installation with:

```powershell
gcal --help
```

Use `.\agent\install.ps1 -GoldeneyeCommand C:\path\to\goldeneye.exe` when Goldeneye is not on `PATH`. Use `-SkipBuild`, `-SkipGlobalLink`, or `-SkipSkills` when you only want part of the setup.

Manual setup is:

```bash
cd agent
pnpm install
pnpm build
```

After building, the executable entrypoint is `dist/main.js`. The package also exposes the `gcal` bin when installed or linked through the package manager.

## Configuration

```bash
export GCAL_BACKEND=goldeneye
export GCAL_GOLDENEYE_COMMAND=/path/to/goldeneye
```

Goldeneye is always selected when `GCAL_BACKEND` is unset. `GCAL_GOLDENEYE_COMMAND` selects its executable and defaults to `goldeneye`. Set it to a full executable path when Goldeneye is not on `PATH`; it is an executable path, not a shell command with arguments.

GCAL starts an on-demand local daemon for Goldeneye by default. The daemon keeps one
Goldeneye MCP session per active project and exits after all sessions remain idle
for 10 minutes. Configure it in `$HOME/.gcal/config.json`:

```json
{
  "daemon": {
    "mode": "auto",
    "idleTimeout": "10m"
  }
}
```

Environment variables override the file. Set `GCAL_DAEMON=off` to restore one
Goldeneye process per GCAL invocation. Set `GCAL_DAEMON_IDLE` to a positive duration
using `ms`, `s`, `m`, or `h`:

```bash
export GCAL_DAEMON=auto
export GCAL_DAEMON_IDLE=10m
```

The daemon uses a local Windows named pipe or a user-only Unix socket under
`GCAL_HOME`; it does not open a TCP port. Benchmark mode bypasses the daemon.
Changing daemon configuration takes effect when the current daemon exits.

The retained compatibility adapter is benchmark-only. Enable it explicitly with `GCAL_BACKEND=benchmark`; only then does GCAL read `GCAL_MCP_COMMAND` and the optional `GCAL_MCP_URL` gateway endpoint:

```bash
export GCAL_BACKEND=benchmark
export GCAL_MCP_COMMAND=/path/to/benchmark-backend
export GCAL_MCP_URL=http://localhost:8767/mcp
```

Run `gcal init` once from a project root. GCAL indexes the directory, reads its Goldeneye project name, and stores the mapping in `$HOME/.gcal/projects.json`. Later project-backed commands resolve the current directory against the most specific registered ancestor.

`GCAL_HOME` overrides the GCAL state directory itself. Its default is `$HOME/.gcal`. Set it to an isolated absolute directory for benchmarks or tests:

```bash
export GCAL_HOME=/tmp/gcal-state
```

`GCAL_PROJECT` remains an optional explicit override for CI, scripts, or commands executed outside a registered project:

```bash
export GCAL_PROJECT=my-indexed-project
```

`init` and `index` can bootstrap without `GCAL_PROJECT`; both use the selected backend. `index` only indexes; `init` indexes and registers the project locally.

Help output also works without `GCAL_PROJECT`:

```bash
node dist/main.js --help
node dist/main.js help inspect
```

## Commands

```bash
gcal init
gcal init ../another-project

gcal search "BookingService" --limit 5
gcal search "cancel booking" --label Method --file "src/.*Service" --qn ".*Booking.*"
gcal search "SecurityConfig|JwtAuthenticationFilter" --limit 5
gcal search "@SpringBootTest"
gcal search "*Test"
gcal search "BookingService" --query "BookingRepository" --query "BookingController"
gcal search "BookingService" --query "BookingRepository" --snippets
gcal search "BookingService" --snippets 5

gcal symbol ".*BookingService.*" --limit 5
gcal symbol "cancel.*" --label Method --qn ".*BookingService.*"

gcal inspect "BookingService" --limit 5
gcal inspect "com.example.BookingService.cancelBooking"

gcal get "com.example.BookingService.cancelBooking"
gcal get "com.example.BookingService.cancelBooking" "com.example.BookingRepository.findActiveBooking"
gcal get "com.example.LargeService.run" --chunk 1
gcal get "com.example.LargeService.run" --chunk 2 --expected-source-sha "<64-lowercase-hex>"

gcal callers "com.example.BookingService.cancelBooking" --depth 1 --limit 20
gcal callees "com.example.BookingService.cancelBooking" --depth 1 --limit 20

gcal workflow "BookingService cancel" --source
gcal workflow "BookingService cancel" --callers --callees
gcal workflow "com.example.BookingService.cancelBooking" --exact --all

gcal arch
gcal status
gcal index .
```

Small single-symbol `gcal get` performs one legacy source call and prints exact source.
When Goldeneye returns typed `SnippetTooLarge`, GCAL performs one manifest call and
prints bounded size, chunk-count, source-SHA, and continuation metadata without
fetching source implicitly. `--chunk <n>` performs one direct 8 KiB chunk call.
`--expected-source-sha` is optional with `--chunk` and must be exactly 64 lowercase
hexadecimal characters; Goldeneye rejects stale source snapshots.

Batch `gcal get <id...>` accepts up to 32 qualified names, performs one direct 8 KiB
chunk call per ID on Goldeneye, and prints stable `# <qualified-name>` source blocks.
One-chunk results remain exact. Multi-chunk results include bounded continuation
metadata. Aggregate stdout remains capped at 48 KiB. Per-ID failures are written to
stderr without discarding successful blocks. Exit status is `0` only when every ID
succeeds and `1` for validation or any per-ID failure. The benchmark compatibility
adapter retains legacy `get` behavior when chunk tools are unavailable.

Search keeps its legacy single-query output when neither `--query` nor `--snippets`
is present. Up to seven repeatable `--query <query>` options add branches (eight
queries total); branches are evaluated in order and results retain stable first rank
using rank-major/query-order merging with qualified-name deduplication. Enhanced
search globally caps candidates at 20.
`--snippets` hydrates the top three candidates, while `--snippets <n>` accepts 1–5.
Goldeneye hydration performs one direct 4 KiB chunk call per candidate. Hydrated
output caps each snippet at 4 KiB and aggregate stdout at 24 KiB; partial chunks
reserve space for continuation metadata. Query and hydration failures are isolated
on stderr; successful results remain available and any partial failure returns exit
status `1`.

The commands intentionally produce compact agent-facing output. Candidate output from `search` and `symbol` defaults to 5 candidates. Relationship output from `callers` and `callees` defaults to depth 1 and 20 rows; use `--depth` and `--limit` to override those bounds. Each relationship row contains the related qualified name, hop, file, and line as four tab-separated fields.

`workflow` performs bounded dependent discovery in one CLI invocation. A broad
argument first searches up to five candidates and selects rank one by default;
`--rank <n>` selects another displayed candidate. `--exact` skips search. Requested
`--source`, `--callers`, and `--callees` hops run concurrently after selection;
`--all` requests all three. Source uses one 8 KiB chunk, relationship output defaults
to depth one and 20 rows per hop, and hard bounds prevent fan-out beyond 20 search
candidates, depth four, or 50 rows per trace. Successful sections survive a failed
hop, but partial workflows exit with status `1`.

`search` treats input as literal-safe by default instead of exposing backend FTS grammar. Pipe-separated alternatives are searched independently, merged in stable branch order, deduplicated by qualified name, and bounded by one global limit. A leading Java annotation marker such as `@SpringBootTest` is normalized to its literal name. A leading wildcard such as `*Test` routes to an escaped suffix symbol match; use `symbol` for other regex searches.

`arch` projects high-signal architecture sections to bounded JSON and omits the full file tree. Use `inspect` when metadata determines whether source is necessary. When an exact-source task already requires the source body, skip `inspect` and call `get` directly.

## Phase 1 Boundary

GCAL does not implement `gcal elect` yet. Its deterministic primitives cover project initialization, search, symbol lookup, inspect, bounded multi-hop workflows, exact source retrieval, call tracing, architecture, status, and indexing.

## Workflow Kit

Reusable agent workflow assets live in `workflow/`:

- `workflow/AGENTS.md`
- `workflow/skills/goldeneye-code-agent-layer/SKILL.md`
- `workflow/skills/goldeneye-code-agent-layer/agents/openai.yaml`
- `workflow/skills/goldeneye-code-agent-layer/agents/claude.md`
