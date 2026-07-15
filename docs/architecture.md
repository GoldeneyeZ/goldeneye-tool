# Goldeneye Architecture

## Decision

Adopt a five-layer Clean Architecture and migrate incrementally. Physical crate
location declares ownership. Cargo package names and public APIs remain stable
until consumers have moved behind explicit boundaries.

## Dependency Direction

```text
domain <- ports <- application <- delivery
            ^             ^
            |             |
         adapters --------+
```

- **Domain** owns graph identities, value objects, invariants, and policy-free
  data structures. It depends on no Goldeneye layer.
- **Ports** owns narrow interfaces required by application use cases. Ports use
  domain types and never expose SQLite, filesystem, Tree-sitter, transport, or
  process details.
- **Application** owns indexing, querying, editing, cross-linking, and project
  administration use cases. It depends only on domain, ports, and application
  modules.
- **Adapters** implement ports for SQLite, source discovery, syntax grammars,
  Git, artifacts, and other external mechanisms.
- **Delivery** translates CLI, MCP, HTTP, and watcher events into application
  requests and composes concrete adapters.

Delivery may depend on every inner layer. Adapters and application are sibling
branches joined through ports; neither may depend on the other.

## Physical Layout

```text
crates/
  domain/
  ports/
  application/
  adapters/
  delivery/
```

Crates remain feature-focused inside a layer. Five layers do not imply five
monolithic crates. Merge a crate only when its separate lifecycle or public
boundary has no demonstrated value.

## Current Migration Debt

`Cargo.toml` records temporary application-to-adapter exceptions. Each entry
must correspond to a real forbidden dependency; stale or newly introduced
violations fail `cargo xtask architecture verify`.

Current debt is concentrated in:

- indexing -> discovery, SQLite store, syntax;
- orchestration -> artifact, discovery, Git, SQLite store, syntax.

## Migration Plan

1. Group crates physically by layer and enforce the dependency graph.
2. Extract ports one use case at a time, starting with project/query reads.
3. Move adapter construction and environment loading into delivery composition.
4. Split the application facade into indexing, querying, editing, and project
   administration services.
5. Remove each migration exception in the same change that introduces its
   replacement port.
6. Consolidate accidental microcrates only after dependencies point inward.
7. Split oversized files along stable responsibilities without widening public
   APIs.

Every step must leave the workspace buildable and independently reversible.

## Validation

- `cargo xtask architecture verify`
- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- existing compatibility and benchmark suites after behavior-affecting moves

Architecture verification is structural evidence. Behavioral tests remain
required because dependency direction alone cannot prove correct use cases.

## Tradeoffs

Ports add mapping and indirection. Introduce them only at volatile external
boundaries; do not abstract deterministic domain algorithms or create
one-method interfaces merely to satisfy a diagram. Physical moves increase
short-term review noise, so package names and APIs stay stable during the first
migration phase.

## Refusal Criteria

Do not merge modules that have independent feature flags, build pipelines,
security boundaries, or release concerns. Do not introduce distributed
services, runtime dependency injection frameworks, or alternative persistence
backends without separate evidence.
