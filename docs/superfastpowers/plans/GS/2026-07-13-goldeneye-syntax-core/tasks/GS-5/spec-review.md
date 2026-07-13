# GS-5 Spec Review

Result: checked

- Reviewed: 2026-07-13 Europe/Paris
- Reviewed range: `9feb49b..39ec323`
- Repair commit: `39ec323`
- Independent reviewer: `/root/gs5_git_repair_worker/gs5_spec_recheck`
- Verdict: CHECKED; no remaining GS-5 specification finding.

## Scope and Evidence

- Reviewed the actual committed range (11 changed files, 1,251 insertions and
  271 deletions), the GS plan Task 5 contract, GS-5 task/context/handoff, prior
  spec review, progression, and the failed final-integration review.
- Accepted the focused and real-pack command evidence already recorded in
  `context.md:235-282`; this one-turn recheck did not rerun the long 1.29 GB
  real-pack gates.
- Confirmed the lock/export provenance update spans the raw pinned Git blobs,
  regenerated 159 grammar hashes and core expectations, and updated commands
  and attribution (`grammars/full-pack.toml:1`,
  `tools/export_grammar_lock.py:1`, `THIRD_PARTY.md:1`, and
  `tasks/GS-5/context.md:235`).

## Requirement Trace

1. The original directory APIs remain, while Git verification/copy obtain the
   commit only from `GrammarPackLock::upstream_commit`; both source kinds enter
   the same private framed hash/copy loop (`crates/goldeneye-syntax/src/pack.rs:196`,
   `crates/goldeneye-syntax/src/pack.rs:204`,
   `crates/goldeneye-syntax/src/pack.rs:240`, and
   `crates/goldeneye-syntax/src/pack.rs:520`).
2. The CLI requires exactly one directory `--source` or the paired
   `--git-repo`/`--git-prefix` form, and absent Git sync streams into an owned
   temporary source before the existing destination-safe materialization path
   (`xtask/src/main.rs:1` and `xtask/src/lib.rs:1`).
3. Git access canonicalizes the repository, disables replacements and lazy
   fetching, verifies the exact commit, parses NUL-delimited `ls-tree`, and
   accepts only modes `100644`/`100755`
   (`crates/goldeneye-syntax/src/pack/git_source.rs:1`).
4. A persistent OID-only `cat-file --batch` session validates the object type,
   declared size, exact byte count, and trailing delimiter; every error/drop
   path closes stdin and kills/reaps the child
   (`crates/goldeneye-syntax/src/pack/git_source.rs:80` and
   `crates/goldeneye-syntax/src/pack/git_source.rs:300`).
5. Directory traversal, atomic create-new writes, existing-pack no-op behavior,
   mismatch rejection, overlap rejection, and partial-output cleanup remain in
   the shared path (`crates/goldeneye-syntax/src/pack.rs:520` and
   `xtask/src/lib.rs:1`). No archive/checkout/full-blob `Vec` path was added.
6. The committed regressions cover CRLF/smudged worktrees, replacement refs,
   non-regular modes, payloads larger than two stream buffers, mixed CLI forms,
   duplicate identical ABI markers, and overlap-window boundary counting
   (`tools/test_export_grammar_lock.py:1`,
   `xtask/tests/grammar_sync.rs:1`, and
   `crates/goldeneye-syntax/src/pack.rs:1`).

## Findings

No active specification findings. The two final-integration failures recorded
in `implementer-handoff.md:12-16` are closed by this range: raw Git-byte parity
is restored and duplicate identical ABI markers no longer satisfy the
exactly-one contract.
