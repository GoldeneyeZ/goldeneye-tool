### Task 5: Verify production separation and full acceptance

<TASK-ID>ABFI-5</TASK-ID>

**Files:**

- Verify only; no planned modifications.

- [ ] **Step 1: Run complete bench test package**

```powershell
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
```

Expected: all tests pass; checks exit `0`.

- [ ] **Step 2: Verify physical one-folder isolation**

```powershell
$outside = git ls-files tools | Where-Object {
  $_ -match '(?i)bench' -and $_ -notlike 'tools/agent-bench/*'
}
if ($outside) {
  $outside
  throw "benchmark runtime file remains outside tools/agent-bench"
}
```

Expected: no output and no exception.

- [ ] **Step 3: Verify Cargo package graph**

```powershell
$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$toolRefs = @($metadata.packages | Where-Object {
  $_.manifest_path -match 'tools[\\/]agent-bench|benchmark-agent|benchmark-competitors' -or
  @($_.targets | Where-Object {
    $_.src_path -match 'tools[\\/]agent-bench|benchmark-agent|benchmark-competitors'
  }).Count -gt 0
})
if ($toolRefs.Count -ne 0) {
  $toolRefs | ConvertTo-Json -Depth 10
  throw "Cargo package graph references benchmark tooling"
}
```

Expected: zero matching packages and no exception.

- [ ] **Step 4: Verify repository hygiene**

```powershell
git diff --check
git status --short
```

Expected:

- `git diff --check` exits `0`.
- Only pre-existing untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` may remain.

- [ ] **Step 5: Record final evidence**

Record:

- Node test count and failures.
- Syntax-check status.
- Old-path grep count.
- Runtime files outside `tools/agent-bench/`.
- Cargo tool-reference count.
- Final commits for ABFI-1 through ABFI-4.
