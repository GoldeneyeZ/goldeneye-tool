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

One `gcal` call should answer one question. Every command accepts the name you
already have, so there is no need to search first to learn an identifier.

| You have | Write |
| --- | --- |
| A type or member name | `Billing`, `Billing.charge`, `com.shop.Billing.charge`, `Billing::charge` |
| A file | `src/app/Billing.java`, or just `Billing.java` |
| A line | `Billing.java:42` (the symbol containing that line) |
| A range | `Billing.java:10-60` (raw lines) |

```bash
gcal init                                   # index and register the project (once)

gcal get Billing.charge Ledger.record       # source; several names, every overload
gcal get Billing                            # whole type; a member outline above 500 lines
gcal get Billing --full                     # whole source whatever the size
gcal get src/app/Billing.java:120-180       # raw line range
gcal outline Billing                        # force the outline

gcal search BookingService cancelBooking    # symbols by name, several queries
gcal search BookingService --snippets 2     # ...with source of the top results
gcal search "*Test" --label Class --file "src/test/.*"
gcal symbol "cancel.*" --qn ".*BookingService.*"   # regular expression on names

gcal callers Billing.charge                 # call graph plus text references
gcal callers src/lib/native.ts              # a file: every file that imports it, and the lines using what it imports
gcal callees Billing.charge --depth 2
gcal impls PaymentGateway                   # implementations and subclasses

gcal grep "retry.max" --file "\.ya?ml$"     # text, grouped by file and symbol
gcal grep "TODO\(.*\)|FIXME"                # a regex, as in rg; literal text when it is not one (-F: literal only)
gcal grep charge Billing.java src/test -C 8 # matching lines with context, like rg -C
gcal grep charge . -C 2                     # the whole repository, files outside the index included
gcal get src/app                            # a directory: its files and their sizes

gcal replace "charge\(" "bill(" src test    # the same edit in every file; prints each changed line
gcal replace -F "a.b" "a.c" . --dry-run     # literal text, preview only
gcal check                                  # what the edit broke or left behind (parse errors, old names still called)
gcal check --types                          # ...and the new type errors (TypeScript: tsc --noEmit against HEAD)
gcal replace "charge\(" "bill(" src -g "*.java" # only the files a glob selects; the others that match are named

gcal arch                                   # directory layout by file count
gcal workflow --js 'const hits = await gcal.search("BookingService cancel"); return gcal.select(hits);'
gcal workflow --file ./investigate.js
gcal workflow --file ./investigate.js --max-calls 64 --timeout-ms 60000
gcal status
gcal index .                                # refresh the index after large edits
```

Output is compact and reusable. Symbol rows are
`Type.member<TAB>Kind<TAB>path:line<TAB>declaration`; source blocks start with
`# Type.member Kind path:start-end`, so they can be passed back to `gcal get` or
used directly as edit targets. Limits default to 5 rows for `search` and
`symbol` and 60 for `callers` and `callees`; override with `--limit`.

Behaviors worth knowing:

- `search` treats input as literal-safe (`A|B` for alternatives, `*Suffix` for
  a suffix match). A query that names no symbol is searched as text in the same
  call, so literals, annotations, and phrases such as `implements Ledger` work.
- `get` reads the working tree. When a file changed after it was indexed, the
  declaration is re-located by text and the block says so; run `gcal index .`
  to refresh search and call-graph results.
- `get` on an ambiguous name prints up to three small candidates, otherwise a
  pick list with `path:line` for each.
- `callers` marks each row `call` (graph edge) or `text` (reference the graph
  did not resolve, such as a call through an interface). Use `--no-text` for
  graph edges only.
- `callers` given a file lists its importers: imports, re-exports, dynamic
  imports and test mocks, under each spelling of the path (alias, relative,
  with or without extension), for JavaScript and TypeScript, Python, C-family
  includes, and assets imported by path. It answers for a path that no longer
  exists, which is how to find what a moved file left behind. In languages
  that import by name (Java, Go, Rust, ...) it answers with the callers of the
  type the file is named after.
- `get` given a symbol together with a type or file too long to print whole
  also prints the members of that type that call the symbol.
- `get --tests` on a symbol no test uses prints the test files nearest to its
  file and the shortest of them whole (up to 80 lines), once per answer.
- `get --tests` also prints the head of the first test file it shows (imports
  and fixtures above the first test, up to 40 lines): what a new test there is
  written from.
- `get --tests` is read as the intent to change the symbols: one that sits in
  a file of at most 150 lines comes with the whole file, and when all its
  tests are in one test file of at most 300 lines that file is printed whole.
- `get --tests` for a method keeps to the test files that name its type; for
  a type, or for a batch of more than three names, it lists where the tests
  are (file and lines) instead of printing them, and prints one short test
  whole with the head of its file, once per answer, as an example of how the
  project writes its tests.
- `search` also lists the files whose name holds a search word.
- `check` without paths goes beyond parsing, from what the index knows: a
  function, method or type that HEAD declares and the working tree no longer
  does is named with the lines of its callers that still use it (a method only
  in files that name its type or call a function returning it); a deleted file
  is named with the files that still import it; and in Java a `new Type(..)`
  on an added line is reported when `Type` declares constructors and none
  takes that number of arguments. None of this is a compiler: it sees names
  and counts, not types.
- `check --types` also runs the project's own type checker. TypeScript only:
  `tsc --noEmit` with the `typescript` of the project's `node_modules` and its
  `tsconfig.json`. When it reports errors, HEAD is exported to a temporary
  directory and type-checked too, and only the errors HEAD does not have are
  printed, those of the changed files first. Two compiler runs take 25 to 30 s
  on a 500-file project. It is off by default: it is a build step.
- `get <file> --tests` lists the test files that import the file with the
  lines that use what they import, and prints the test file whole when it is
  the only one (up to 300 lines); for a language that imports by name, the
  tests of the type the file declares.
- `grep` without `--limit` shows 30 matching symbols, or all of them when the
  backend returned every match (up to 90).
- `callers` says when the text search read fewer places than matched.
- A lookup that finds nothing (`# not found`, `# ambiguous`, `# no text
  matches`) exits 0, so a chain written with `&&` goes on; `replace`, `check`
  and backend failures keep a failing exit status.
- Several commands can be chained in one shell call: `gcal get A; gcal callers A.run`.

`gcal get <name> --chunk <n> [--expected-source-sha <sha256>]` remains available
for reading one oversized symbol in fixed 8 KiB chunks.

## Phase 1 Boundary

GCAL does not implement `gcal elect` yet. Its deterministic primitives cover project initialization, symbol and text search, source retrieval, outlines, call tracing, subtypes, layout, status, and indexing.

`workflow` runs true JavaScript so one CLI invocation can loop, branch, select search
results, and decide later calls from earlier evidence. Supply exactly one of `--js`
or `--file`. The async body receives `gcal.search`, `gcal.select`, `gcal.source`, `gcal.trySource`,
`gcal.callers`, `gcal.callees`, `gcal.tryCallers`, and `gcal.tryCallees`
(`gcal.get` is an alias); return a JSON-serializable value for stdout. Prefer `--file`
for multi-line workflows; use inline `--js` only for short bodies. JavaScript can use
normal control flow and `Promise.all`:

```js
const hits = await gcal.search("authentication", { limit: 10 });
const evidence = [];

for (const hit of hits) {
  const source = await gcal.trySource(hit.qualifiedName);
  if (!source.ok) continue;
  if (!source.source.includes("token")) continue;

  const [callers, callees] = await Promise.all([
    gcal.callers(hit.qualifiedName, { depth: 1, limit: 20 }),
    gcal.callees(hit.qualifiedName, { depth: 1, limit: 20 }),
  ]);
  evidence.push({ hit, source, callers, callees });
}

return evidence;
```

Defaults allow 32 backend calls and 30 seconds; flags can raise these to hard caps
of 128 calls and 120 seconds. Per-operation bounds remain 20 search candidates,
one 8 KiB source chunk, trace depth four, and 50 trace rows. Returned stdout is
capped at 48 KiB and captured console output at 8 KiB.

Workflow code is trusted Node.js, not a security sandbox. The worker timeout and
call budget protect process liveness; they do not prevent filesystem, network,
environment, or secret access. Run only code you trust.

`search` treats input as literal-safe by default instead of exposing backend FTS grammar. Pipe-separated alternatives are searched independently, merged in stable branch order, deduplicated by qualified name, and bounded by one global limit. A leading Java annotation marker such as `@SpringBootTest` is normalized to its literal name. A leading wildcard such as `*Test` routes to an escaped suffix symbol match; use `symbol` for other regex searches.

`arch` projects high-signal architecture sections to bounded JSON and omits the full file tree. Use `inspect` when metadata determines whether source is necessary. When an exact-source task already requires the source body, skip `inspect` and call `get` directly.


## Workflow Kit

Reusable agent workflow assets live in `workflow/`:

- `workflow/AGENTS.md`
- `workflow/skills/goldeneye-code-agent-layer/SKILL.md`
- `workflow/skills/goldeneye-code-agent-layer/agents/openai.yaml`
- `workflow/skills/goldeneye-code-agent-layer/agents/claude.md`
