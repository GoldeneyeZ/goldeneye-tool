# GFP-5 Context

Status: Implemented in `b2ccf4a`. Per the active plan-progression bypass, GFP-5 did not run task-level spec or code-quality reviews; its evidence is queued for the single goal-level audit.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Starting baseline: `3800986`
- Implementation commit: `b2ccf4a`
- Upstream commit: `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- Full-pack lock SHA-256: `ce668d1c07d4f7dd72fd8f167f94d218bfc933a1ccd9ffa52277354968c950c1`

Implemented outcome:

- The existing `rust` CI job remains the unchanged Linux/Windows/macOS default matrix with cache-free workspace formatting, Clippy, and tests.
- A separate Linux `full-pack` job depends on the default matrix, checks out the exact audited upstream SHA, installs Rust 1.97.0 with Rustfmt/Clippy, and runs `cargo fetch --locked` before crossing one explicit offline boundary.
- After `CARGO_NET_OFFLINE=true`, the job reproduces the lock, materializes and re-verifies the pack, checks the generated provider and license ledger, runs native/full-only Clippy and tests, runs the mixed collision sentinel, enforces the full-only feature tree, and links the release runtime-audit binaries.
- The full job uses explicit `GOLDENEYE_GRAMMAR_PACK_DIR` state. It has no cache action, broad restore key, or cache-hit-as-verification behavior.
- `xtask/tests/full_pack_ci.rs` guards the default/full job split, acquisition/offline ordering, exact commands, absence of full dependencies from default CI, operator claims, current lock hash, 159 grammar ledger rows, and two native-support license rows.
- `docs/full-grammar-pack.md` provides copyable PowerShell and POSIX acquisition, reproduction, sync, verification, feature, recovery, and regression commands.
- `THIRD_PARTY.md` now distinguishes metadata/materialization, GFP runtime evidence, and Phase 6 packaging. It states that core-only is not 160-ID evidence and that no upstream application C or bundled upstream Tree-sitter runtime is linked.
- The deterministic license ledger was regenerated at the current lock hash. It contains 159 grammar rows plus two `common` native-support license rows.

Accepted inventory and claim boundaries:

- The pack has 159 grammar groups, one native-support group, 914 total compilation/license assets, 160 declared IDs, 159 available IDs, 157 unique callable factories, one unavailable `nim` row, and two ObjectScript orphan sources.
- Full factories and scanner exports are namespaced under `goldeneye_full_`. Full-only, default core-only, and mixed artifact feature graphs are documented separately.
- The shared `common` native-support assets and their two direct license paths are explicit in the ledger and operator/third-party documentation.
- The bounded MSVC-only COBOL derivation is documented as exact-hash/structure guarded, limited to two proven array bounds in an `OUT_DIR` copy, fail-closed, and cache preserving.
- All-ID tests prove factory availability, link, locked ABI, parser acceptance, and basic lifecycle coverage. They do not claim broad behavioral conformance; Phase 6 remains responsible for release archives, bundled license texts, platform/compiler evidence, and final binary self-audit.

TDD and deterministic evidence:

- RED: `cargo test -p xtask --test full_pack_ci` failed 0/5 because the full job/operator guide were absent, third-party counts were stale, and the checked-in ledger had no native-support section.
- Focused GREEN: the same CI/documentation contract passed 5/5.
- `python tools/test_export_grammar_lock.py` passed 20/20.
- Lock reproduction reported reproducible; provider generation and license-ledger generation reported current.
- The workflow parses as YAML with jobs `rust` and `full-pack`; the full job has nine steps. No `ack elect` text or external GitHub state mutation was introduced.

Integration evidence:

- Initial env-cleared lane: formatting, workspace Clippy, and all 40 workspace test-suite summaries passed.
- Offline full lane: native Clippy passed; native tests passed 20/20; full-only syntax Clippy/tests passed; mixed all-features tests passed; the feature tree contained the compiled full crate and none of the five maintained core grammar crates.
- Release no-run linked all full-only syntax test executables. The first optimized native build outlived the Context Mode five-minute RPC, continued in the original process, and produced the release binary; an identical cached command then returned exit 0 with complete Cargo evidence.
- Final env-cleared lane: all 40 workspace test-suite summaries passed again; `git diff --check` passed.

Observational baselines, not acceptance ceilings:

- Cached debug-profile full sequence: 34.70 seconds.
- First release `full_grammars` executable appearance: approximately 512.81 seconds from the observed Cargo process start; cached evidence rerun: 0.83 seconds.
- Materialized source cache: 1,255,766,459 bytes (1,197.59 MiB), 915 files.
- Shared workspace `target`: 21,343,583,100 bytes (20,354.83 MiB), including prior debug/release artifacts; this is not a clean-build or distribution size.
- Linked release `full_grammars` test executable: 229,386,240 bytes (218.76 MiB).

Deviations/blockers:

- No unresolved blocker or implementation deviation remains. The original plan's “159-entry ledger” wording was superseded by the accepted GFP-3 blocker resolution: 159 grammar rows plus two rows for the one native-support group. The regenerated ledger records that current authoritative state.
- The prose contract initially treated wrapped Markdown as same-line text. It now normalizes whitespace only for prose, preserving byte-exact workflow commands, ordering, lock hash, and ledger rows.
- The workflow was validated locally but was not remotely dispatched, honoring the instruction not to change external GitHub state.
