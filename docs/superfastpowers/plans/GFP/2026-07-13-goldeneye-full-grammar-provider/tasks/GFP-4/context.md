# GFP-4 Context

Status: Pending.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`

Scope:

- Add the opt-in `full-grammar-pack` syntax feature and safe `FullGrammarProvider`.
- Make the maintained core grammar dependencies explicit behind default feature `core-grammars`.
- Add typed ABI-drift handling and return locked full-pack provenance for supported lookups.
- Audit all 159 supported IDs and exercise both full-only and mixed core/full runtime graphs.
- Gate existing core-provider tests so the full-only lane does not link maintained core grammar crates.

Constraints:

- Default behavior remains unchanged and cache-independent.
- Simultaneous core and full activation must remain supported as the symbol-collision sentinel.
- ObjectScript is absent from runtime queries; `nim` and unknown IDs return typed unsupported errors.
- Provider lookup relies on the safe native registry and preserves requested language IDs.
- ABI conversion is checked, and locked ABI mismatches are rejected precisely.
- The task begins only after GFP-3 has passed both review gates.

Required gates:

- Focused RED test for the missing full provider and feature.
- Full-only runtime tests and Clippy with the verified grammar cache and offline Cargo.
- Mixed all-features link test plus feature-tree proof that full-only excludes the five core grammar crates.
- Complete cache-free default formatting, Clippy, tests, release build, and diff check.

Evidence: Pending. No implementation, test, review, or commit evidence has been recorded.
