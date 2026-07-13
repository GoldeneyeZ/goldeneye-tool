# GFP-5 Context

Status: Pending.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`

Scope:

- Add a separate Linux full-pack CI job while preserving the existing default Linux/Windows/macOS matrix.
- Add tracked CI/documentation contract tests for offline ordering, exact upstream acquisition, regeneration, compilation, runtime, and feature-tree claims.
- Document exact local full-pack acquisition, verification, feature operation, recovery, cardinalities, and packaging boundaries.
- Clarify third-party evidence so core-only builds are never presented as 160-language evidence.

Constraints:

- Use exact upstream commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c` and Rust 1.97.0 in the full-pack CI lane.
- Complete dependency and upstream acquisition before setting the offline boundary.
- Treat materialized-cache verification as mandatory; a cache hit is never verification.
- Preserve default core-only CI and require explicit cache and feature configuration for full-pack work.
- Distinguish GFP verification from Phase 6 release-license packaging and avoid unsupported release ceilings.
- The task begins only after GFP-4 has passed both review gates.

Required gates:

- Focused RED CI/documentation contract test.
- CI contract plus lock, provider-registry, and license-ledger regeneration checks.
- Fresh default, full compiled/provider, mixed-link, full-only feature-tree, release no-run, and final default regression lanes.
- Observational elapsed-time and size baselines without converting them into unsupported acceptance limits.

Evidence: Pending. No implementation, test, review, baseline, or commit evidence has been recorded.
