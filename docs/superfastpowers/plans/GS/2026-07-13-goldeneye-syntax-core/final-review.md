# Goldeneye Syntax Core Final Integration Review

**Reviewed range:** `9c0cee8..6853e056ecf4ea21d369b54fd526c75d021e3e3f`  
**Reviewed head:** `6853e056ecf4ea21d369b54fd526c75d021e3e3f`  
**Result:** **failed**

The five task packages and their spec/code-quality records are administratively
complete, but fresh inspection of the combined code and required real-source
gate found two Important defects and one Minor contract gap. The plan is not
ready to advance.

## Important Findings

### 1. Export, verification, and synchronization do not agree on the bytes being locked

- `tools/export_grammar_lock.py:441` hashes immutable bytes read from the pinned
  Git objects, as required by the exact-commit/raw-content contract.
- `tools/export_grammar_lock.py:30` still pins core hashes produced from a
  Windows checkout after `core.autocrlf=true` expanded LF to CRLF.
  `grammars/full-pack.toml:18` and all other committed `source_hash` values were
  generated from those transformed worktree bytes.
- `crates/goldeneye-syntax/src/pack.rs:195` and `xtask/src/lib.rs:70` verify and
  synchronize filesystem bytes, so they accept this CRLF checkout while the
  hardened exporter rejects the committed lock.

The upstream checkout was clean at the exact declared commit
`2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`; no content change explains the
difference. All 907 assets differ from their Git blobs solely by LF-to-CRLF
expansion. All 159 committed grammar hashes match the Windows worktree and zero
match the pinned Git tree. For Ada, the framed Git-object hash is
`fe745430ec54b5c325ce94f94473855fdedde38d9f98e4cd01d5431ef438ff0e`, while
the committed/worktree hash is
`c37ebf2b1ac15bdedd2978396864f54dd268dc047ecdb8c34c1222b9b918c3b9`.

Consequently the required command
`python tools/export_grammar_lock.py --check ...` exits 1 with an Ada source
hash mismatch, while `cargo xtask grammars verify ...` exits 0 against the same
checkout. The deterministic exact-commit provenance claim in
`THIRD_PARTY.md:36` is therefore not currently true.

Acceptance criteria for repair:

1. Define and enforce one cross-platform byte source consistent with the task:
   raw bytes from the pinned Git commit, without line-ending normalization.
2. Regenerate all 159 lock hashes and the exporter core-hash expectations from
   that source.
3. Make exporter check, verify, and sync consume or require the same byte-stable
   input (for example, an explicitly validated `core.autocrlf=false` checkout
   or exact Git-object materialization).
4. Add a regression covering an `autocrlf` checkout, then rerun exporter
   `--check`, verify, and sync against the same pinned input; every command in
   the plan's metadata/materialization gate must exit 0.

Merely updating `EXPECTED_CORE_HASHES` and `full-pack.toml` is insufficient:
the current filesystem verifier would then reject the CRLF checkout and the
three operations would still disagree.

### 2. Core grammar metadata tests do not check the dependency manifest

`crates/goldeneye-syntax/tests/core_grammars.rs:45` hardcodes the same package
and version strings as `crates/goldeneye-syntax/src/grammar.rs:152`, but neither
copy is checked against the exact dependency declarations at
`crates/goldeneye-syntax/Cargo.toml:15`. A future grammar dependency bump could
leave snapshots advertising the old revision while every existing metadata
test remains green, weakening the grammar-drift guard used by reparsing and
locators.

Repair by parsing/checking the exact manifest pins in the test or generating
provider metadata and assertions from one source of truth. Retain explicit
assertions for all five packages and six runtime grammar IDs.

## Minor Finding

### Duplicate identical ABI markers are accepted

`tools/export_grammar_lock.py:420` stores `LANGUAGE_VERSION` matches in a set,
so two identical markers collapse to one value and pass the `len(...) == 1`
check at line 431. Direct invocation with two `LANGUAGE_VERSION 14` definitions
was accepted, contrary to the Task 5 requirement for exactly one marker.

Count marker occurrences rather than distinct values, without double-counting
matches in the 1024-byte streaming overlap, and add an exporter regression.

## Integration Audit

- Progression records GS-1 through GS-5 as complete, with implementer, spec,
  and code-quality status checked. Every required `context.md`,
  `spec-review.md`, and `code-quality.md` was present and reviewed.
- Implementation commits remain task-scoped. Later tasks did not overwrite the
  earlier engine/locator/inspection implementations; public exports and
  dependencies compose as planned. The full pack remains explicitly pre-GFP
  metadata/materialization, not an unimplemented claim of 160-language runtime
  support.
- Fresh GS-1--GS-4 focused verification passed 53/53 tests: domain 7,
  discovery 2, and syntax grammar/diagnostic/locator/inspection suites 44.
- GS-5 focused tests passed: exporter 3/3, grammar-lock 7/7, grammar-sync 11/11,
  and xtask library 1/1. These suites do not catch the real-checkout byte-source
  disagreement above.
- Independent lock inspection otherwise confirmed 159 grammars, 160 language
  bindings, 907 assets, ABI histogram `13=9, 14=78, 15=72`, 159 available IDs,
  Nim explicitly unavailable, 157 unique bound grammars, and the two explicit
  ObjectScript orphans. Every grammar has a direct `LICENSE` and `parser.c`, and
  the asset allowlist is limited to `LICENSE`, `*.c`, `*.h`, and `*.inc`.
- Cargo dependency closure and `THIRD_PARTY.md` entries agree on package
  versions/licenses; no separate dependency or license omission was found.
  Provenance remains blocked by Important finding 1.
- `git diff --check 9c0cee8..6853e056e` passed. The worktree was clean and HEAD
  equaled the reviewed commit before this requested review artifact was added;
  no unreviewed implementation change was present.

## Final Decision

**Failed.** Close both Important findings, add the ABI-marker regression, rerun
the real pinned-checkout metadata/materialization gate and controller gates, and
obtain fresh independent spec/code-quality review before repeating final
integration review.
