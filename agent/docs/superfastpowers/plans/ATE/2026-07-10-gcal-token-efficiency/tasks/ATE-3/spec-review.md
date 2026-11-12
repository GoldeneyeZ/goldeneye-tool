# Specification Review: ATE-3

Status: checked

Reviewed commit: `aade35f`

## Evidence

- Fixture includes all required scalar/section fields, 21 packages, 500 file-tree entries, and 50 routes.
- Normalizer retains the three scalars, projects only the seven allowed sections, and caps each section at 20.
- Gateway requests the exact seven high-signal aspects with the configured project and normalizes the response.
- Tests assert the projected value and explicitly exclude `file_tree` and `routes`.
- Focused verification: `pnpm vitest run tests/normalize.test.ts tests/gatewayClient.test.ts` — 17 passed.

No missing, extra, or misunderstood requirement found.
