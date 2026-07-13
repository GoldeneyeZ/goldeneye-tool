# Goldeneye Syntax Core Final Integration Review

- Result: failed
- Reviewed range: `9c0cee8..52cb046`
- Reviewed head: `52cb046e8403c9d810d4e965087f060e35d6eeb9`
- Review date: 2026-07-13

## Important Finding

### A chunk boundary inside the ABI digits rejects one valid marker

`tools/export_grammar_lock.py:416` scans each chunk plus a 1024-byte overlap.
The filter at `tools/export_grammar_lock.py:430-434` counts any regex match
whose end enters the new chunk. If a boundary splits `LANGUAGE_VERSION 14`
as `LANGUAGE_VERSION 1 | 4`, the first window records ABI `1` and the next
window records ABI `14`. The exact-one check at
`tools/export_grammar_lock.py:437` then rejects a parser containing only one
marker.

The regression at `tools/test_export_grammar_lock.py:97` splits inside the
`LANGUAGE_VERSION` identifier, so it does not cover this numeric-token split.
An independent probe exercised all 27 split points in
`#define LANGUAGE_VERSION 14\n`; split 26 alone failed with
`ExportError: direct parser must contain exactly one ABI marker`.

This violates the explicit requirement to reject duplicate identical markers
without double-counting a single boundary-crossing marker. Defer a match that
ends at a non-final window boundary until the next chunk (or EOF), retain exact
occurrence counting across the overlap, and add a digit-split regression
(preferably an all-split-points regression).

## Prior Failed-Review Closure History

1. **Exact Git bytes versus CRLF checkout — closed.** Commit `39ec323`
   regenerated all 159 hashes from the pinned commit's Git blobs and added a
   shared Git source session. Export, verify, and sync now use the lock's sole
   revision, disable replacement objects and lazy fetches, accept only
   `100644`/`100755` blobs, validate the persistent `cat-file --batch`
   protocol, and stream absent-destination sync directly into an owned
   temporary pack. Directory mode and its existing safety behavior remain.
2. **Five manifest pins versus six provider IDs — closed.** Commit `be307f2`
   parses the five exact `=version` dependencies, checks all six runtime IDs
   (including TypeScript and TSX sharing one package), and rejects synthetic
   drift independently for every pin.
3. **Duplicate identical ABI markers — partially closed, then reopened by this
   review.** Two identical markers are now rejected and the existing
   identifier-split boundary fixture passes, but the digit-split probe above
   proves the full boundary requirement is not met.

## Integration Audit

- `plan-progression.md` records GS-1 through GS-5 complete with implementer,
  specification, and code-quality checks. Every task context/review was read;
  GS-1, GS-2, and GS-5 handoffs are resolved and no current failed or unchecked
  task-local review remains.
- The actual combined diff and both repair ranges were inspected. Changes after
  `821a0d9` do not modify provider, snapshot/diagnostic, locator, inspection,
  domain, discovery, or public runtime source, so the GS-1 through GS-4 runtime
  implementations were not overwritten by the pack repair.
- The pack repair otherwise fails closed for exact commit/object identity,
  replacement refs, lazy fetches, non-regular Git modes, malformed/truncated
  batch protocol, path overlap, mismatched existing packs, and owned temporary
  cleanup. Absent Git sync hashes and copies each blob through one stream and
  creates no intermediate checkout/archive.
- CLI parsing rejects mixed and incomplete directory/Git modes. The real lock,
  Cargo dependency closure, `THIRD_PARTY.md`, retained per-grammar licenses,
  and pre-GFP metadata/materialization claim are coherent.
- Before this required review artifact was edited, HEAD was exactly
  `52cb046e8403c9d810d4e965087f060e35d6eeb9` and the worktree had zero
  staged, unstaged, or untracked entries.

## Verification Evidence

Fresh focused checks run during this review:

- `cargo test -p goldeneye-syntax --test core_grammars`: 6 passed.
- `python -m unittest tools.test_export_grammar_lock`: 6 passed.
- `cargo test -p goldeneye-syntax --test grammar_lock`: 7 passed.
- `cargo test -p xtask --bin xtask`: 2 passed.
- `cargo test -p xtask --test grammar_sync git_source_`: 4 passed.
- Runtime provider/diagnostic/locator/inspection focus: 46 passed; later repair
  ranges contain no changes to those implementations.
- Exhaustive ABI split probe: 27 split points checked, one failure at
  `LANGUAGE_VERSION 1 | 4`.

The controller's fresh full HEAD gate also reported format, clippy, release
build, and 175 workspace tests passing; exporter `--check` reproducible; Git
verification at 159 grammars / 907 assets; existing-pack sync current; the
CRLF-smudged directory rejected; and raw Git materialization containing LF
bytes. Those broad gates do not exercise the failing digit split above.

## Final Decision

**FAILED.** The Git-byte and manifest-pin repairs are integrated, and no other
Critical or Important finding remains, but the ABI streaming scanner still
violates one explicit repaired requirement. Fix the Important finding and run a
fresh final integration review before marking GS checked.
