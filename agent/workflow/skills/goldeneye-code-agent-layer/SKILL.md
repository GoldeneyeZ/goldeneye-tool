---
name: goldeneye-code-agent-layer
description: Use GCAL over Goldeneye for compact code discovery - symbol search, exact source, outlines, callers, callees, subtypes, text search, and project layout.
---

# Goldeneye Code Agent Layer

One `gcal` call should answer one question. An extra call costs more than the
bytes it saves, so name the target directly and batch.

## When to Use GCAL

Pick the surface that answers in one call, and never ask the same question
through both.

| Need                                                  | Use              |
| ----------------------------------------------------- | ---------------- |
| How a flow works; who calls or implements something   | `gcal`            |
| Source of several named symbols; every use of a name  | `gcal`            |
| The same edit in many files                           | `gcal replace`    |
| Read or edit one file whose path you already have     | plain file tools |

## Pointing at Code

Every command accepts the name you already have. Never search only to learn an
identifier.

| You have              | Write                                                   |
| --------------------- | ------------------------------------------------------- |
| A type or member name | `Billing`, `Billing.charge`, `com.shop.Billing.charge`  |
| A file                | `src/app/Billing.java`, or just `Billing.java`          |
| A line                | `Billing.java:42` (the symbol containing that line)     |
| A range               | `Billing.java:10-60` (raw lines)                        |
| A directory           | `src/app` (its files and their sizes), `.` for the root |

## Commands

| Need                               | Command                                                             |
| ---------------------------------- | ------------------------------------------------------------------- |
| Source of known symbols            | `gcal get A.method B.other` (every overload is included)             |
| A symbol you are about to change   | `gcal get A.method --tests`: its source and the tests that use it (small files whole; with no test of it, the nearest test files and a short one whole). Edit from this answer; do not read the files again. |
| A file and the tests that import it | `gcal get src/lib/module.py --tests` (test files, the lines using what they import) |
| A whole type or file               | `gcal get Type File.ext` (a member outline above 500 lines)          |
| A name you are unsure of           | `gcal search name otherName`; add `--snippets` to include source     |
| Who calls it                       | `gcal callers Type.member` (call graph plus text references)         |
| Who imports a file                 | `gcal callers src/lib/module.ts` (every importer and how it is used) |
| What it calls                      | `gcal callees Type.member --depth 2`                                 |
| Implementations and subclasses     | `gcal impls Type`                                                    |
| A literal, config key, or phrase   | `gcal grep "cancel.*Booking"` (regex as in rg; `-F` literal only)    |
| Matching lines with context        | `gcal grep text File.ext some/dir -C 8` (like `rg -C`)               |
| The same edit in many files        | `gcal replace 'old\(' 'renamed(' src tests` (prints changed lines)   |
| Syntax errors after editing        | `gcal check` (parses every file that differs from HEAD)              |
| Project layout                     | `gcal arch`                                                          |
| Later hops depend on earlier ones  | `gcal workflow --file <path>` or `gcal workflow --js <code>` (below)  |

## Rules

- Batch: pass several names to one `get` or `search`, and chain different
  commands in one shell call (`gcal get A; gcal callers A.run`).
- Rows end in `path:line`. Reuse them as locators and as edit targets.
- `search` falls back to text when no symbol matches; do not repeat it with `grep`.
- `get Type` returns the whole type; do not follow it with reads of its members.
- For every use of a name with its surrounding lines, use one `grep ... -C n`
  instead of a search followed by range reads.
- A rename or a signature change across many files is one `gcal replace` per
  pattern (a regex with `$1` groups, `-F` for literal text, `--dry-run` to
  preview); patch by hand only what the pattern cannot express. Add `-g '*.py'`
  to keep data files and documents under the same directories unchanged.
- Before moving or renaming a module, `gcal callers <its path>` lists every file
  that imports it, each way its path is written (alias, relative, in a mock) and
  the lines that use what each file imports. It still answers for the old path
  after the file has moved.
- `get` given a symbol together with a type or file too long to print whole
  (`gcal get Billing.refund BillingTests`) also prints the members of that type
  that call the symbol; do not read them again.
- Put `--tests` on the one to three symbols you will change. On a type or on a
  larger batch it only lists where the tests are.
- After edits in several files, end with `gcal check` in the same shell call as
  `git status`: it names every line that no longer parses, in any language.
- In PowerShell, write patterns in single quotes. Inside double quotes `\"` ends
  the string, and the whole command line then prints nothing and runs nothing.
  For a quote character inside a pattern write `\x22` (double) or `\x27`
  (single): `gcal grep 'from [\x22\x27]\./native[\x22\x27]'`.
- A name `get` cannot resolve is answered from the text when possible (a macro,
  a symbol added since indexing, the only symbol of that name under another
  owner); read the header line before asking again.
- Stop discovery once the task has enough evidence.
- Do not run `gcal status` or `gcal --help` unless blocked by project state or syntax.
- `get` and `grep` also reach files outside the index (docs, build scripts,
  files created since indexing). Never repeat a query through direct Goldeneye
  MCP tools.

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

Goldeneye calls reuse an on-demand local GCAL daemon holding one backend session
per active project; it exits after 10 minutes idle (`GCAL_DAEMON_IDLE=10m`). Set
`GCAL_DAEMON=off` only when direct per-command Goldeneye startup is required.
Benchmark mode (`GCAL_BACKEND=benchmark`) always bypasses the daemon and starts
the backend once per command.
