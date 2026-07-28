### Task 3: Add private package boundary and operator README

<TASK-ID>ABFI-3</TASK-ID>

**Files:**

- Modify: `tools/agent-bench/isolation.test.mjs`
- Create: `tools/agent-bench/package.json`
- Create: `tools/agent-bench/README.md`

- [ ] **Step 1: Add failing package-contract test**

Append to `tools/agent-bench/isolation.test.mjs`:

```javascript
test("bench package is private ESM with stable commands", async () => {
  const manifest = JSON.parse(
    await readFile(path.join(BENCH_ROOT, "package.json"), "utf8"),
  );

  assert.equal(manifest.private, true);
  assert.equal(manifest.type, "module");
  assert.equal(manifest.engines.node, ">=20");
  assert.deepEqual(manifest.scripts, {
    test: "node --test",
    check: "node --check ./bin/benchmark-agent-tasks.mjs && node --check ./bin/benchmark-competitors.mjs",
    "benchmark:agents": "node ./bin/benchmark-agent-tasks.mjs",
    "benchmark:competitors": "node ./bin/benchmark-competitors.mjs",
  });
});
```

- [ ] **Step 2: Run test to verify RED**

```powershell
node --test tools/agent-bench/isolation.test.mjs
```

Expected: FAIL with `ENOENT` for `tools/agent-bench/package.json`.

- [ ] **Step 3: Add package manifest**

Create `tools/agent-bench/package.json`:

```json
{
  "name": "@goldeneye/agent-bench",
  "private": true,
  "type": "module",
  "engines": {
    "node": ">=20"
  },
  "scripts": {
    "test": "node --test",
    "check": "node --check ./bin/benchmark-agent-tasks.mjs && node --check ./bin/benchmark-competitors.mjs",
    "benchmark:agents": "node ./bin/benchmark-agent-tasks.mjs",
    "benchmark:competitors": "node ./bin/benchmark-competitors.mjs"
  }
}
```

- [ ] **Step 4: Add README**

Create `tools/agent-bench/README.md` with:

```markdown
# Goldeneye agent benchmark tools

All benchmark runtime code, configs, tasks, graders, and fixtures live in this directory. Production Cargo packages do not import this directory.

## Requirements

- Node.js 20 or newer
- Benchmark-specific external repositories, commands, and environment variables required by the selected config

## Commands

From repository root:

```powershell
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
npm --prefix tools/agent-bench run benchmark:agents -- <arguments>
npm --prefix tools/agent-bench run benchmark:competitors -- <arguments>
```

Direct entrypoints:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs <arguments>
node tools/agent-bench/bin/benchmark-competitors.mjs <arguments>
```

## Layout

- `bin/`: benchmark entrypoints
- `configs/`: benchmark configurations
- `graders/`: grading code and injected fixtures
- `tasks/`: agent task prompts
- root modules: shared harness, timing, provenance, qualification, reporting, and snapshot logic

## Inputs and outputs

Entrypoint arguments and environment variables remain defined by each runner and config. Generated benchmark evidence belongs under configured output directories, normally `target/agent-bench/`; generated evidence is not production runtime input.

## Production boundary

Goldeneye production builds use Cargo packages under `crates/`. Removing `tools/agent-bench/` removes benchmark runtime tooling without changing the Cargo package graph. Legacy paths `tools/benchmark-agent-tasks.mjs` and `tools/benchmark-competitors.mjs` no longer exist.
```

- [ ] **Step 5: Verify GREEN**

```powershell
node --test tools/agent-bench/isolation.test.mjs
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
```

Expected: all tests pass; both syntax checks exit `0`.

- [ ] **Step 6: Commit package boundary**

```powershell
git add -- tools/agent-bench/isolation.test.mjs tools/agent-bench/package.json tools/agent-bench/README.md
git commit -m "docs(bench): define isolated package boundary"
```
