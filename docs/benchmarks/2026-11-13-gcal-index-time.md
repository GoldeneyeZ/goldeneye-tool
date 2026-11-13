# Where the preparation time of the GCAL lane goes, and the time of a full index

Date: 2026-11-13. Follows `2026-11-13-gcal-final-build-three-repetitions.md`,
which had found that the whole harness invocation took the same in both lanes
(7,201 s against 7,161 s over 35 runs each) although the agent itself was
faster with GCAL, and had explained it wrongly. No agent run was made for
this note; every number is a timing on this machine.

## What the time was

Not a cost per run. The harness keeps one index snapshot per task and builds
it again when the GCAL build changes (`snapshot_refreshed` in the report). The
three repetitions followed two builds, so that happened before 23 of the 35
GCAL runs.

| GCAL runs | Count | Time outside agent, grader and maintenance |
|---|---|---|
| Snapshot built again | 23 | 1,518 s (10 to 152 s each) |
| Snapshot ready | 12 | 67 s (3 to 13 s each) |
| Plain shell, for comparison | 35 | 129 s (3.7 s each) |

Over the 12 pairs whose snapshot was ready, the whole invocation takes 1,946 s
with GCAL against 2,362 s with plain shell (0.82), and the agent 1,412 s
against 1,902 s (0.74).

Building a snapshot is: `git worktree add`, a full `gcal init`, a checkpoint,
a copy of the index and two verifications of it. The harness records it in
`target/agent-bench/<task>/preparation.json`. Before this note, on the hard
disk the benchmark works on:

| Task | Whole preparation | of which `gcal init` |
|---|---|---|
| Spring, three tasks | 117 to 129 s | 70 to 74 s |
| LOD | 59 s | 53 s |
| agentsview | 55 s | 49 s |
| codebase-memory-mcp | 64 s | 29 s |
| goldeneye-tool | 25 s | 21 s |
| terax, two tasks | 16 to 18 s | 13 to 16 s |
| abyssal, ViV-Wallet, PyFinance | 8 to 13 s | 5 to 10 s |

This is also what a user pays once per repository, so the rest of the note is
about `gcal init`.

## Where a full index spends its time

Timed with temporary timers in the indexer and the store (not committed), on
clones on the NVMe disk, so that the disk is not what is measured.

Three Spring modules: 2,614 files, 47,860 nodes, 85,992 edges, 48.4 s.

| Phase | Before | After |
|---|---|---|
| Discover the files | 4.5 s | 3.6 s |
| Read and hash | 0.4 s | 0.4 s |
| Parse (the only phase on several threads) | 1.3 s | 1.3 s |
| Assemble the graph | 16.0 s | 3.2 s |
| of which resolve calls and relations | 15.5 s | 2.1 s |
| Write to the store | 21.0 s | 7.7 s |
| of which nodes | 11.7 s | 3.9 s |
| of which edges | 5.2 s | 1.2 s |
| of which commit | 3.5 s | 2.4 s |
| Rest (start, checkpoint when the store closes) | about 5 s | about 4 s |
| **`gcal init`** | **48.4 s** | **20.5 s** |

LOD (730 files, 113,899 nodes, 117,997 edges): the store took 34.6 s of
43.1 s, the nodes alone 23.7 s; after, 14.8 s of 22.2 s.

## The three causes, and what was changed (commit `690fc40`)

**The store wrote through a page cache of 2 MiB.** A node row goes into the
table, four indexes and the full-text table; an edge row into the table and
three indexes. With SQLite's default cache the pages of those indexes were
evicted and read back for nearly every row: 244 µs per node. To see which
setting mattered, the rows of the Spring index were loaded into an empty
database with the same schema under different settings (Node's SQLite, one
transaction, as the store does it):

| Setting | Load |
|---|---|
| As the store did it | 15.7 s |
| Page cache of 256 MiB | 4.8 s |
| Full-text table built after the rows | 16.3 s |
| Foreign keys off | 12.7 s |
| Secondary indexes created after the rows | 9.7 s |
| Page cache of 256 MiB, full-text table after | 5.3 s |
| Page cache of 256 MiB, indexes and full-text table after | 5.6 s |

Only the cache matters, and nothing adds to it. The store now sets the cache
to 256 MiB for the time of a whole-project replacement and puts back the
value it found. Foreign keys, triggers and the order of work are as before.

**Call resolution tried every unresolved call again.** The resolver looped up
to three times over the calls it had not resolved, as long as a pass had
resolved something. A call is resolved from the definitions and from the
imports of its file, and neither changes while calls are resolved: the second
pass could never resolve anything, and it ran over the calls that cost the
most, those with many candidates and no answer. It is one pass. 15.5 s to
9.3 s on Spring.

**Every comparison normalized both names again.** A call is compared with
every definition of its name by up to six rules, and each rule called helpers
that normalized the definition's qualified name (291 characters on average in
the Spring index) by copying it several times, and the imports of the file
the same way, for every candidate. The normalized qualified name, the owner
type, the class-and-method tail and the lookup key are now computed once per
definition, and the names of an import once per file. 9.3 s to 2.1 s on
Spring.

## Result

`gcal init` on clones on the NVMe disk, one run each:

| Repository | Before | After |
|---|---|---|
| Spring, three modules (StringUtils task) | 45 to 50 s (three runs) | 21.4 s |
| Spring, three modules (redaction task) | 45 s | 21.1 s |
| LOD | 43 s | 22.7 s |
| agentsview (Go) | 30 s | 17.7 s |
| codebase-memory-mcp (C) | 15 s | 10.3 s |
| goldeneye-tool (Rust) | 6 s | 3.4 s |
| terax (TypeScript) | 4 s | 2.7 s |
| PyFinance, ViV-Wallet | 1 s | 0.7 s, 0.8 s |

On the benchmark's hard disk, in the harness's own preparation
(`--prepare-snapshot`, one run each, a fresh worktree every time):

| Task | `gcal init` before | after | Whole preparation before | after |
|---|---|---|---|---|
| Spring StringUtils | 74 s | 43 s | 129 s | 108 s |
| LOD | 53 s | 48 s | 59 s | 86 s |
| agentsview | 49 s | 45 s | 55 s | 57 s |
| codebase-memory-mcp | 29 s | 36 s | 64 s | 73 s |
| goldeneye-tool | 21 s | 20 s | 25 s | 46 s |
| terax fuzzy | 16 s | 14 s | 18 s | 19 s |

There the gain is Spring's alone. The other repositories did not get faster,
and the whole preparation got slower on three of them: on that disk the time
is the disk (files read right after `git worktree add`, the index and its
copy written to the same disk), and single runs differ by more than the
change. The benchmark's preparation stays what it was until its worktrees and
caches are on the other disk.

## Verification

- The graph is the same: for ten repositories (two Spring tasks, LOD,
  agentsview, codebase-memory-mcp, terax, goldeneye-tool, abyssal, ViV-Wallet,
  PyFinance) the nodes and the edges of an index made by the new binary were
  compared, row by row with their properties, with an index of the same clone
  made by the binary before the change: no node and no edge lost or new, on
  47,860 + 85,992 rows for Spring down to 852 + 1,279 for PyFinance.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace`: 101 suites, 491 tests, none failed. Two tests were
  added: the string checks that replace the helpers agree with the strings
  they stand for on 1,331 combinations, and a project replacement leaves the
  page cache as it found it.

## Found and not changed

- **The resolver takes the first 100,000 calls of a project.** The three
  Spring modules reach that limit. Calls are sorted by file before the cut,
  so the calls of the files last in path order are not resolved by this
  resolver. How many are cut was not counted, and what it costs `gcal callers`
  was not measured. Until now the limit also bounded the resolver's time,
  which is no longer the reason to keep it at that value.
- **Rows are wide.** The project identifier is in every row and every index
  entry, and qualified names carry it too: 228 MB for 133,852 rows of the
  Spring clone, 361 MB for LOD, more than the 256 MiB the cache now has,
  which is why LOD's store phase is still 14.8 s. In the benchmark, where the
  project path is shorter, the same indexes are 126 MB and 195 MB.
- **Discovery takes 3.6 s on Spring** to keep 2,614 files of the repository.
- **The write is done twice**: into the write-ahead log, then into the
  database when the store closes (about 4 s for Spring).
- **Memory.** The cache can hold 256 MiB more during a full index, on top of
  the graph in memory (470 MB resident for Spring before the change). The
  peak after the change was not measured.

## Not verified

- No agent run was made on the new binary.
- The times are single runs, on one machine.
- The time of an incremental index (one file changed) was not measured, before
  or after.
