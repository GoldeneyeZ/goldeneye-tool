# GFP-2 Implementer Handoff

- Status: pending
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`

## Scope

Add exact `exported_symbol` metadata, regenerate/reproduce the lock, generate the safe prefixed registry and deterministic license ledger, and establish the default-empty native crate.

## Constraints

Use streamed bounded parsing; preserve audited counts/hashes/ABIs; reject duplicate or malformed symbols and orphan mappings; keep generated output deterministic and cache-free by default.

## Required Gates

Run the exporter, regeneration, generator `--check`, focused Rust/Python tests, default-empty crate check, workspace tests, and diff check listed in `task.md`.

## Handoff

No repair brief exists. Begin only after GFP-1 completes, using TDD from Step 1.

## Evidence

Pending; implementation has not started.
