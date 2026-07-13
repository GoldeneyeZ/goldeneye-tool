# GS-5 Code Quality Review

Result: unchecked after final integration reopen

Source: `../../final-review.md`

## Prior Checked Review

Result: checked after repair

Reviewed: 2026-07-13 12:41 Europe/Paris
Reviewed range: `76b618b..4b02e99`
Implementation commit: `4b02e9962a089e1b44bc8471d323f522d517ee77`
Independent reviewer: `/root/gs_5_worker/gs5_quality_review`
Independent final verdict: CHECKED; no remaining actionable finding.

## Evidence Reviewed

- Shared lock validation, framed hashing, same-handle copy, source/destination
  safety, atomic publication/cleanup, pack-state re-verification, and tests.
- Deterministic exporter, immutable pinned Git inventory/blob reads, full lock,
  ABI/license/provenance checks, dependency closure, and legal ledger.
- Python snapshot tests 3/3 and byte-identical exporter `--check`; grammar-lock
  tests 7/7; xtask unit 1/1; sync tests 11/11; real 159-grammar/907-asset
  verify and existing-pack sync.
- Windows workspace format, clippy `-D warnings`, all tests, release build, and
  diff check passed. The actual pack implementation and Unix regressions also
  typechecked for `x86_64-unknown-linux-gnu` through an isolated manifest.

## Findings and Repairs

1. Medium, closed: non-Unix Rust source access originally checked pathname
   components and then reopened the full path, leaving an intermediate Windows
   junction swap race. The final implementation anchors at the stable
   filesystem/volume root, opens every source-root and asset directory with
   capability-relative no-follow traversal, opens the final asset no-follow,
   holds one source-root handle per operation, and hashes/copies that final
   `File`. `cap-primitives` uses handle-relative `NtCreateFile` on Windows.
2. Medium, closed: the exporter originally inventoried worktree paths and later
   reopened them, leaving a concurrent reparse/symlink substitution race. It
   now inventories the exact pinned commit with `git ls-tree -r -z --long`,
   accepts only regular Git blob modes at read time, and streams bytes from one
   `git cat-file --batch` process. Tests prove worktree replacement cannot
   affect the pinned bytes and mode `120000` is rejected.

The independent recheck confirmed both findings are closed and found no new
correctness, determinism, cleanup, dependency/legal, or maintainability issue.
The final-amend recheck also confirmed that a copied environment disables Git
replacement objects consistently for `run_git`, `ls-tree`, and persistent
`cat-file --batch`; the regression proves original committed bytes win over a
configured replacement commit without changing subprocess lifecycle or output
ordering.

Residual platform note: Unix code was target-typechecked rather than executed
on this Windows host. Windows symlink regression execution depends on local
symlink privilege/developer-mode availability.
