# GS-5 Spec Review

Result: failed (reopened by final integration review)

Active findings: raw Git-byte parity and duplicate identical ABI marker acceptance.
Source: `../../final-review.md`

## Prior Checked Review

Result: checked after repair

Reviewed: 2026-07-13 12:41 Europe/Paris
Reviewed range: `76b618b..4b02e99`
Implementation commit: `4b02e9962a089e1b44bc8471d323f522d517ee77`
Independent reviewer: `/root/gs_5_worker/gs5_independent_review`
Independent final verdict: PASS; no remaining GS-5 specification finding.

## Evidence Reviewed

- GS-5 task contract, shared lock schema, deterministic exporter/full lock,
  verify/sync implementation, legal ledger, and focused tests.
- Exporter `--check`, real 159-grammar/907-asset verification, existing-pack
  no-op sync, workspace clippy with `-D warnings`, workspace tests, and release
  build all exited successfully.
- The final replacement-object regression suite ran 3 tests and passed.
- The actual `pack.rs` and `grammar_lock.rs` Unix paths typechecked for
  `x86_64-unknown-linux-gnu` through an isolated manifest; the full workspace
  cross-check requires a Linux C cross-compiler for existing Tree-sitter crates.

## Findings and Repairs

1. Important, closed: lock validation originally accepted arbitrary asset types
   and non-direct licenses. It now allows only nested `*.c`, `*.h`, `*.inc`,
   exactly one direct `LICENSE`, and requires direct `parser.c` plus license
   membership. Regressions cover `README.md`, nested license, and missing parser.
2. Important, closed: source validation originally separated pathname checks
   from ordinary opens. The final implementation anchors at the stable
   filesystem/volume root, traverses every source-root and asset directory with
   capability-relative no-follow opens, opens the final asset no-follow, and
   hashes/copies from that same handle. Regressions cover final and intermediate
   symlinks, including the Windows build.
3. Important, closed: exact-commit export originally allowed Git replacement
   refs to substitute another commit tree while preserving the requested SHA.
   Every exporter Git subprocess (`run_git`, `ls-tree`, and persistent
   `cat-file --batch`) now receives `GIT_NO_REPLACE_OBJECTS=1`. The regression
   installs `refs/replace` between distinct commits and proves export still
   reads the original commit's blob bytes.

The independent final recheck confirmed all repairs, ran the 3-test exporter
suite, and found no missing, extra, or misunderstood GS-5 requirement.
