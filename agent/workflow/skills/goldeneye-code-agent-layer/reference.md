# GCAL Reference

Read on need; `SKILL.md` has what every task uses.

## Trusted JavaScript Workflow

`gcal workflow` executes a true async JavaScript body in one worker. Supply exactly
one of `--js <code>` or `--file <path>`. The body can loop, branch, use
`Promise.all`, and call `gcal.search`, `gcal.select`, `gcal.source` (`gcal.get` is an
alias), `gcal.trySource`, `gcal.callers`, `gcal.callees`, `gcal.tryCallers`, and
`gcal.tryCallees`. `gcal.trySource`
returns `{ ok: true, ...sourceFields }` or `{ ok: false, error }`, so one source
miss does not reject a batch. Read successful source text from `.source` directly.
Return a JSON-serializable value.
Prefer `--file` for multi-line programs; reserve inline `--js` for short bodies to
avoid shell-quoting corruption.
Use a targeted `filePattern` search option and deduplicate candidates by `filePath`
when a broad query spans code, tests, docs, or resource files. Search results may
include project/module candidates without source; use `gcal.trySource` for batches.
`filePattern` accepts regex or common `*`/`**` glob spelling.
Use `gcal.tryCallers` or `gcal.tryCallees` for optional traces; successful results
are `{ ok: true, edges }` and misses are `{ ok: false, error }`.

```js
const hits = await gcal.search("authentication", { limit: 10 });
const evidence = [];
for (const hit of hits) {
  const source = await gcal.trySource(hit.qualifiedName);
  if (!source.ok) continue;
  if (!source.source.includes("token")) continue;
  const callers = await gcal.callers(hit.qualifiedName, { depth: 1, limit: 20 });
  evidence.push({ hit, source, callers });
}
return evidence;
```

Keep defaults unless the task requires more: 32 backend calls and 30 seconds.
Flags can raise these to hard caps of 128 calls and 120 seconds. Operation caps are
20 search candidates, one 8 KiB source chunk, trace depth four, and 50 trace rows.
Returned output is capped at 48 KiB; captured console output is capped at 8 KiB.

This is trusted Node.js, not a security sandbox. Worker limits protect liveness but
do not block filesystem, network, environment, or secret access. Execute only code
you trust. Keep loop termination explicit even though the wall-clock timeout exists.

## Project State

Run `gcal init` once from the project root to index it and register its Goldeneye
project name in `$HOME/.gcal/projects.json`. Project-backed commands resolve the
current directory against the most specific registered ancestor. `GCAL_PROJECT` is
an optional explicit override for CI or scripts. Set `GCAL_HOME` to an isolated
absolute state directory for benchmarks or tests.

`gcal get` reads the working tree, so source is current even after edits; it warns
when the index lags behind. Run `gcal index .` after large edits to refresh search
and call-graph results. `gcal index` only indexes; `gcal init` also registers.
`get` and `grep` also reach files outside the index (docs, build scripts, files
created since indexing).

Goldeneye calls reuse an on-demand local GCAL daemon holding one backend session
per active project; it exits after 10 minutes idle (`GCAL_DAEMON_IDLE=10m`). Set
`GCAL_DAEMON=off` only when direct per-command Goldeneye startup is required.
Benchmark mode (`GCAL_BACKEND=benchmark`) always bypasses the daemon and starts
the backend once per command.

## Command Details

- `get --tests`: the source and the tests that use it (small files whole; with no
  test of it, the nearest test files). Put it on the one to three symbols whose
  behaviour you will change; on a type or a larger batch it only lists where the
  tests are. On a file it lists the test files that import it.
- `get` given a symbol together with a type or file too long to print whole also
  prints the members of that type that call the symbol.
- `callers <symbol>`: how the files import it (the usual statement with its
  count, every other spelling with its place), a `def` row, the `call` rows of
  the graph and the `text` rows. A name few symbols share is answered for all of
  them. `callers <path>` still answers for the old path after a file has moved.
- `replace`: a regex with `$1` groups; `\n` in the replacement is a line break;
  `-F` for literal text. It prints each changed line, where else the pattern
  still matches, and the lines where a bare word was only in a comment or string.
- `check`: lines that no longer parse, callers that still use a renamed or
  removed symbol, imports of a deleted file, a name a file still uses after its
  import was removed, `new Type(..)` calls no constructor takes (Java).
  `check --types` adds the type errors the change introduced (TypeScript).
- `search` falls back to text when no symbol matches; do not repeat it with `grep`.
