# Agent bench folder isolation design

## Goal

Make all tracked benchmark runtime tooling removable or extractable as one directory:

```text
tools/agent-bench/
```

Production Rust packages, targets, and behavior remain unchanged.

## Current state

- `tools/agent-bench/` contains 47 tracked files: core modules, tests, configs, graders, fixtures, and tasks.
- Two runtime entrypoints remain outside:
  - `tools/benchmark-agent-tasks.mjs`
  - `tools/benchmark-competitors.mjs`
- `tools/benchmark-agent-tasks.mjs` imports seven modules from `tools/agent-bench/`.
- Production `crates/**` contain no bench-tool references.
- Cargo metadata contains 22 workspace packages, zero bench targets, and zero tool-path dependencies.
- Root `Cargo.toml` contains no benchmark-tool reference.

## Considered approaches

### A. Hard relocation — selected

Move both entrypoints into `tools/agent-bench/bin/`, update every tracked path reference, and add a private Node package manifest plus README.

Benefits:

- True one-directory isolation.
- No production Cargo coupling.
- Clear entrypoints and ownership.

Cost:

- Existing external callers using old paths must update.

### B. Compatibility wrappers — rejected

Keep thin scripts at old paths that forward to new entrypoints.

Rejected because deleting `tools/agent-bench/` would leave benchmark runtime files under `tools/`.

### C. Manifest-only boundary — rejected

Add documentation and package metadata without moving entrypoints.

Rejected because physical isolation remains incomplete.

## Target structure

```text
tools/agent-bench/
├── bin/
│   ├── benchmark-agent-tasks.mjs
│   └── benchmark-competitors.mjs
├── configs/
├── graders/
├── tasks/
├── package.json
├── README.md
└── existing core modules and tests
```

## Migration

1. Add a failing isolation-contract test.
2. Move `tools/benchmark-agent-tasks.mjs` to `tools/agent-bench/bin/benchmark-agent-tasks.mjs`.
3. Move `tools/benchmark-competitors.mjs` to `tools/agent-bench/bin/benchmark-competitors.mjs`.
4. Rewrite moved agent-task runner imports:
   - `./agent-bench/core.mjs` → `../core.mjs`
   - Apply same rewrite to its other six local bench imports.
5. Update every tracked reference to both old paths, including tests, active docs, and historical benchmark plans.
6. Add `tools/agent-bench/package.json`:
   - `"private": true`
   - `"type": "module"`
   - Node engine requirement
   - scripts for tests and both entrypoints
7. Add `tools/agent-bench/README.md` documenting commands, folder ownership, inputs, outputs, and production separation.
8. Do not add compatibility wrappers.

## Interfaces

New direct commands:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs
node tools/agent-bench/bin/benchmark-competitors.mjs
```

Package-local equivalents:

```powershell
npm --prefix tools/agent-bench run benchmark:agents
npm --prefix tools/agent-bench run benchmark:competitors
```

Existing CLI arguments, environment variables, output formats, and exit behavior remain unchanged.

## Error handling and compatibility

- Missing configuration, invalid arguments, subprocess failures, and benchmark failures retain current behavior.
- Old entrypoint paths intentionally stop existing.
- All tracked callers migrate in the same change.
- README highlights the breaking path change.
- No production code or Cargo manifest may import or reference the bench directory.

## Testing

### RED

Add an isolation-contract test proving:

- Both new `bin/` entrypoints exist.
- Both old top-level entrypoints do not exist.
- Agent-task runner local imports resolve inside `tools/agent-bench/`.
- Production `crates/**` and root `Cargo.toml` contain no bench-tool references.

Run before relocation and confirm expected failure.

### GREEN

After relocation:

```powershell
node --test tools/agent-bench/*.test.mjs
node --check tools/agent-bench/bin/benchmark-agent-tasks.mjs
node --check tools/agent-bench/bin/benchmark-competitors.mjs
```

Then verify:

- No tracked reference contains `tools/benchmark-agent-tasks.mjs`.
- No tracked reference contains `tools/benchmark-competitors.mjs`.
- Cargo metadata reports no bench targets or tool-path packages.
- `git diff --check` passes.

## Acceptance criteria

- All benchmark runtime code lives under `tools/agent-bench/`.
- Exactly zero benchmark runtime files remain directly under `tools/`.
- Every tracked caller uses new paths.
- Existing bench tests pass.
- Both moved entrypoints pass Node syntax checks.
- Production Cargo package graph remains bench-free.
- No compatibility wrappers remain.

## Out of scope

- Production ZIP/archive generation.
- Cross-platform release packaging.
- Benchmark behavior or scoring changes.
- Rust production-code changes.
- Benchmark artifact relocation under `target/`.
