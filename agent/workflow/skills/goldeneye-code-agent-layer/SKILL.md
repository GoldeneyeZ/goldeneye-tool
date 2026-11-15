---
name: goldeneye-code-agent-layer
description: Use GCAL over Goldeneye for compact code discovery - symbol search, exact source, outlines, callers, callees, subtypes, text search, and project layout.
---

# Goldeneye Code Agent Layer

One `gcal` call should answer one question. An extra call costs more than the
bytes it saves: name the target directly, pass several names to one command,
and chain commands in one shell call (`gcal get A; gcal callers A.run`).

Use `gcal` when the answer spans files; use plain file tools for one file whose
path you already have. Never ask the same question through both.

## Pointing at Code

Every command takes the name you already have. Never search only to learn an
identifier.

- A symbol: `Billing`, `Billing.charge`, `com.shop.Billing.charge`
- A file or a directory: `src/app/Billing.java`, `Billing.java`, `src/app`
- A line: `Billing.java:42` (the symbol containing it); a range: `Billing.java:10-60`

## Commands

| Need | Command |
| --- | --- |
| Source of symbols, types, files | `gcal get A.method B Type File.ext` (every overload; a member outline above 500 lines) |
| A symbol you will change, with its tests | `gcal get A.method --tests` |
| Who calls it; before a rename, move, removal or signature change | `gcal callers Type.member other` (call graph plus text references: the declaration, the imports, every call to its end) |
| Who imports a file | `gcal callers src/lib/module.ts` |
| What it calls; implementations | `gcal callees Type.member`, `gcal impls Type` |
| Text, with context | `gcal grep "cancel.*Booking" src -C 8` (regex as in rg; `-F` literal) |
| A name you are unsure of | `gcal search name otherName` |
| The same edit in many files | `gcal replace 'old\(' 'renamed(' src tests -g '*.py'` (`$1` groups, `--dry-run`) |
| What an edit broke or left behind | `gcal check`, after edits in more than one file |
| Project layout | `gcal arch` |
| Later hops depend on earlier ones | `gcal workflow --file <path>` or `gcal workflow --js <code>` (`reference.md`) |

## Rules

- Rows end in `path:line`; reuse them as locators and as edit targets.
- An answer says what it left out and how to ask for it: read its `#` lines
  before asking again.
- Edit from what an answer printed; do not read the same lines again. Get a
  file whole when you will edit several places of it.
- Stop discovery once the task has enough evidence.
- Do not run `gcal status` or `gcal --help` unless blocked by project state or syntax.
- In PowerShell, write patterns in single quotes, and a quote character inside
  a pattern as `\x22` (double) or `\x27` (single).
- No project for the current directory: run `gcal init` once from the project
  root. `reference.md` has project state, index freshness and the daemon.
