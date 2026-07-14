# Goldeneye performance session prompt

Copy the prompt below into a new session.

```text
Goal: improve Goldeneye performance against codebase-memory-mcp using the checked-in reproducible benchmark.

Workspace: D:/Dev/IdeaProjects/goldeneye-tool
Benchmark corpus: D:/Dev/IdeaProjects/terax-ai

No brainstorming or TDD ceremony. Work directly, preserve correctness, and keep existing user changes.

Start by inspecting git status/diff, then run a baseline:

node tools/benchmark-competitors.mjs --repo D:/Dev/IdeaProjects/terax-ai --cold-runs 1 --warmups 3 --samples 20 --out target/benchmarks/baseline.json

The comparator defaults to `codebase-memory-mcp` from PATH. Override with
`--comparator <absolute-path>` when needed.

Read `target/benchmarks/baseline.json`. Compare latency together with response
cardinality, content bytes, and wire bytes. Do not claim a latency win when the
engines returned materially different work.

Pick the highest-impact bottleneck, implement one focused optimization, verify
correctness, then rerun with:

node tools/benchmark-competitors.mjs --repo D:/Dev/IdeaProjects/terax-ai --cold-runs 1 --warmups 3 --samples 20 --out target/benchmarks/after.json

Report before/after p50, p95, index time, response size/cardinality, tests, and
remaining gaps. Continue while a safe measurable optimization remains.

Known candidates, in likely ROI order:

1. `Store::replace_project_graph`: remove redundant per-edge
   `ensure_node_exists` SELECTs; input validation already verifies endpoints.
2. Cache `nodes_by_id` and `nodes_by_qualified_name` in `ProjectGraph`; reuse in
   trace and snippet resolution.
3. Cache compact architecture summaries per graph generation.
4. Add simple-Cypher LIMIT fast paths; preserve or explicitly document total
   semantics.
5. Move fast-mode semantic check before `list_nodes`; reuse computed token
   vectors during semantic indexing.

Before completion run relevant focused tests plus:

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check

Do not commit unless explicitly requested.
```

## Quick smoke run

Use this after small changes:

```powershell
node tools/benchmark-competitors.mjs `
  --repo D:/Dev/IdeaProjects/terax-ai `
  --cold-runs 1 `
  --warmups 1 `
  --samples 3 `
  --startup-wait-ms 0 `
  --skip-build `
  --out target/benchmarks/smoke.json
```

Remove `--skip-build` whenever source changed.
