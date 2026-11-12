### Task 5: Document Contracts and Verify the Complete Change

<TASK-ID>ATE-5</TASK-ID>

**Files:**
- Modify: `tests/installScript.test.ts`
- Modify: `README.md`
- Modify: `docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md`

- [ ] **Step 1: Write a failing README contract assertion**

```typescript
expect(readme).toContain("sole model-facing code-discovery surface");
expect(readme).toContain("defaults to 5 candidates");
expect(readme).toContain("defaults to depth 1 and 20 rows");
expect(readme).toContain("omits the full file tree");
```

- [ ] **Step 2: Run the installer test and verify it fails**

Run: `pnpm vitest run tests/installScript.test.ts`

Expected: FAIL because README does not yet describe the new contracts.

- [ ] **Step 3: Update README command and workflow documentation**

Document these exact behaviors:

- search/symbol default to 5 candidates;
- callers/callees default to depth 1 and 20 rows, with `--depth` and `--limit` overrides;
- relationship rows are `related qualified name`, `hop`, `file`, `line`;
- architecture output is projected and omits the full file tree;
- GCAL is the sole model-facing code-discovery surface while available;
- the Windows installer migrates the old direct-MCP managed instruction block;
- exact-source tasks skip inspect and call get directly.

- [ ] **Step 4: Update the design spec only if implementation names differ**

Compare the implemented option names, output columns, architecture sections, and installer marker behavior with the spec. If any implemented name differs, edit the spec to use the shipped name; otherwise leave the spec unchanged.

- [ ] **Step 5: Run formatting checks on changed files**

Run: `pnpm exec prettier --check src tests workflow README.md docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md`

Expected: PASS. If formatting fails, run `pnpm exec prettier --write` on only the reported changed files and rerun the check.

- [ ] **Step 6: Run the complete project verification**

Run: `pnpm check`

Expected: ESLint passes, all Vitest tests pass, and TypeScript build passes.

- [ ] **Step 7: Inspect the final diff for accidental scope expansion**

Run: `git diff --check && git status --short && git diff --stat main...HEAD`

Expected: no whitespace errors; only the files listed in this plan are changed; no `gcal elect`, batching, telemetry, generated build output, or live MCP test dependency is present.

- [ ] **Step 8: Commit documentation and verification updates**

```bash
git add README.md tests/installScript.test.ts docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md
git commit -m "docs: explain bounded GCAL workflows"
```
