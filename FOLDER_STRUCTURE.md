# Goldeneye-tool folder structure

Complete Git-tracked repository tree: 497 files.

Present but intentionally collapsed: `.git/` (Git metadata), `.upstream/` (ignored upstream checkout), `target/` (generated Rust build artifacts). Expanding these would add large disposable/internal trees.

```text
goldeneye-tool/
|-- .cargo/
|   `-- config.toml
|-- .github/
|   `-- workflows/
|       |-- ci.yml
|       |-- packaging.yml
|       `-- release.yml
|-- crates/
|   |-- adapters/
|   |   |-- goldeneye-artifact/
|   |   |   |-- src/
|   |   |   |   |-- lib.rs
|   |   |   |   `-- port.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-discovery/
|   |   |   |-- data/
|   |   |   |   `-- languages.tsv
|   |   |   |-- src/
|   |   |   |   |-- ignore_rules.rs
|   |   |   |   |-- language.rs
|   |   |   |   |-- lib.rs
|   |   |   |   |-- policy.rs
|   |   |   |   |-- port.rs
|   |   |   |   `-- walker.rs
|   |   |   |-- tests/
|   |   |   |   |-- fixtures/
|   |   |   |   |   `-- discovery/
|   |   |   |   |       `-- manifest.tsv
|   |   |   |   |-- discovery_parity.rs
|   |   |   |   |-- domain_ids.rs
|   |   |   |   |-- ignore_parity.rs
|   |   |   |   |-- language_parity.rs
|   |   |   |   `-- upstream_parity.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-full-grammars/
|   |   |   |-- src/
|   |   |   |   |-- generated.rs
|   |   |   |   `-- lib.rs
|   |   |   |-- tests/
|   |   |   |   `-- compiled_registry.rs
|   |   |   |-- build.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-git/
|   |   |   |-- src/
|   |   |   |   |-- lib.rs
|   |   |   |   `-- port.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-grammar-pack/
|   |   |   |-- src/
|   |   |   |   |-- git_source.rs
|   |   |   |   `-- lib.rs
|   |   |   |-- tests/
|   |   |   |   `-- materialized_pack.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-store/
|   |   |   |-- src/
|   |   |   |   |-- adr_traces_port.rs
|   |   |   |   |-- adr.rs
|   |   |   |   |-- crosslink_port.rs
|   |   |   |   |-- edit_port.rs
|   |   |   |   |-- git_history_port.rs
|   |   |   |   |-- index_port.rs
|   |   |   |   |-- lib.rs
|   |   |   |   |-- project_administration_port.rs
|   |   |   |   |-- query_port.rs
|   |   |   |   |-- repository_factory.rs
|   |   |   |   |-- schema.rs
|   |   |   |   `-- semantic_index_port.rs
|   |   |   |-- tests/
|   |   |   |   |-- adr_traces.rs
|   |   |   |   |-- git_history.rs
|   |   |   |   |-- repository_factory.rs
|   |   |   |   `-- store.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-syntax/
|   |   |   |-- src/
|   |   |   |   |-- edit_port.rs
|   |   |   |   |-- engine.rs
|   |   |   |   |-- full_grammar.rs
|   |   |   |   |-- grammar.rs
|   |   |   |   |-- inspect.rs
|   |   |   |   |-- lib.rs
|   |   |   |   `-- locator.rs
|   |   |   |-- tests/
|   |   |   |   |-- fixtures/
|   |   |   |   |   `-- compact-inspection.json
|   |   |   |   |-- core_grammars.rs
|   |   |   |   |-- diagnostics.rs
|   |   |   |   |-- full_grammars.rs
|   |   |   |   |-- grammar_lock.rs
|   |   |   |   |-- inspect.rs
|   |   |   |   `-- locators.rs
|   |   |   `-- Cargo.toml
|   |   `-- goldeneye-tree-sitter-index/
|   |       |-- src/
|   |       |   |-- extract/
|   |       |   |   |-- calls.rs
|   |       |   |   |-- classify.rs
|   |       |   |   |-- graph.rs
|   |       |   |   |-- imports.rs
|   |       |   |   |-- mod.rs
|   |       |   |   `-- relations.rs
|   |       |   |-- error.rs
|   |       |   |-- language_specs.rs
|   |       |   `-- lib.rs
|   |       `-- Cargo.toml
|   |-- application/
|   |   |-- goldeneye-crosslink/
|   |   |   |-- src/
|   |   |   |   `-- lib.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-edit/
|   |   |   |-- src/
|   |   |   |   |-- durable/
|   |   |   |   |   `-- recovery.rs
|   |   |   |   |-- durable.rs
|   |   |   |   |-- lib.rs
|   |   |   |   `-- path_auth.rs
|   |   |   |-- tests/
|   |   |   |   |-- durable.rs
|   |   |   |   |-- operations.rs
|   |   |   |   |-- path_auth.rs
|   |   |   |   |-- planning.rs
|   |   |   |   |-- stale.rs
|   |   |   |   `-- validation.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-index/
|   |   |   |-- src/
|   |   |   |   |-- edit_port.rs
|   |   |   |   |-- enrichment.rs
|   |   |   |   |-- hybrid.rs
|   |   |   |   |-- identity.rs
|   |   |   |   |-- lib.rs
|   |   |   |   |-- project_graph.rs
|   |   |   |   |-- service.rs
|   |   |   |   `-- types.rs
|   |   |   |-- tests/
|   |   |   |   |-- support/
|   |   |   |   |   `-- full_language_fixtures.rs
|   |   |   |   |-- fast_index.rs
|   |   |   |   |-- full_language_corpus.rs
|   |   |   |   `-- index_modes.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-query/
|   |   |   |-- assets/
|   |   |   |   `-- nomic/
|   |   |   |       |-- code_tokens.txt
|   |   |   |       |-- code_vectors.bin
|   |   |   |       |-- LICENSE
|   |   |   |       |-- NOTICE
|   |   |   |       `-- README.md
|   |   |   |-- src/
|   |   |   |   |-- cypher/
|   |   |   |   |   |-- ast.rs
|   |   |   |   |   |-- evaluate.rs
|   |   |   |   |   |-- lexer.rs
|   |   |   |   |   |-- mod.rs
|   |   |   |   |   |-- parser.rs
|   |   |   |   |   `-- projection.rs
|   |   |   |   |-- engine/
|   |   |   |   |   |-- mod.rs
|   |   |   |   |   |-- resolve.rs
|   |   |   |   |   |-- search.rs
|   |   |   |   |   |-- snippet.rs
|   |   |   |   |   `-- trace.rs
|   |   |   |   |-- ast_profile.rs
|   |   |   |   |-- lib.rs
|   |   |   |   |-- rotsq.rs
|   |   |   |   |-- search_code.rs
|   |   |   |   |-- semantic_query.rs
|   |   |   |   |-- semantic.rs
|   |   |   |   |-- similarity.rs
|   |   |   |   `-- types.rs
|   |   |   |-- tests/
|   |   |   |   |-- common/
|   |   |   |   |   `-- mod.rs
|   |   |   |   |-- architecture.rs
|   |   |   |   |-- contracts.rs
|   |   |   |   |-- cypher_core_parity.rs
|   |   |   |   |-- cypher_multiclause_parity.rs
|   |   |   |   |-- cypher_scalar_case_parity.rs
|   |   |   |   |-- cypher_union_unwind_parity.rs
|   |   |   |   |-- query_graph.rs
|   |   |   |   |-- search_code.rs
|   |   |   |   |-- search.rs
|   |   |   |   |-- semantic_search.rs
|   |   |   |   |-- snippet.rs
|   |   |   |   `-- trace.rs
|   |   |   `-- Cargo.toml
|   |   `-- goldeneye-services/
|   |       |-- src/
|   |       |   |-- adr_traces.rs
|   |       |   |-- edit.rs
|   |       |   |-- git.rs
|   |       |   `-- lib.rs
|   |       |-- tests/
|   |       |   |-- adr_traces.rs
|   |       |   |-- git_changes.rs
|   |       |   `-- services.rs
|   |       `-- Cargo.toml
|   |-- delivery/
|   |   |-- goldeneye-bootstrap/
|   |   |   |-- src/
|   |   |   |   `-- lib.rs
|   |   |   |-- tests/
|   |   |   |   `-- service_indexer.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-cli/
|   |   |   |-- src/
|   |   |   |   |-- lib.rs
|   |   |   |   `-- main.rs
|   |   |   |-- tests/
|   |   |   |   |-- stdio_adr_traces.rs
|   |   |   |   |-- stdio_services.rs
|   |   |   |   `-- stdio.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-compat-tests/
|   |   |   |-- src/
|   |   |   |   `-- lib.rs
|   |   |   |-- tests/
|   |   |   |   `-- frozen_contract.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-http/
|   |   |   |-- src/
|   |   |   |   |-- assets.rs
|   |   |   |   |-- backend.rs
|   |   |   |   |-- lib.rs
|   |   |   |   `-- server.rs
|   |   |   |-- build.rs
|   |   |   `-- Cargo.toml
|   |   |-- goldeneye-mcp/
|   |   |   |-- src/
|   |   |   |   |-- lib.rs
|   |   |   |   |-- protocol.rs
|   |   |   |   |-- server.rs
|   |   |   |   |-- tools.rs
|   |   |   |   `-- transport.rs
|   |   |   |-- tests/
|   |   |   |   |-- gcal_tools.rs
|   |   |   |   |-- adr_traces.rs
|   |   |   |   `-- git_changes.rs
|   |   |   `-- Cargo.toml
|   |   `-- goldeneye-watcher/
|   |       |-- src/
|   |       |   `-- lib.rs
|   |       |-- tests/
|   |       |   `-- watcher.rs
|   |       `-- Cargo.toml
|   |-- domain/
|   |   `-- goldeneye-domain/
|   |       |-- src/
|   |       |   |-- graph.rs
|   |       |   |-- lib.rs
|   |       |   `-- syntax.rs
|   |       |-- tests/
|   |       |   |-- language_id.rs
|   |       |   `-- syntax_types.rs
|   |       `-- Cargo.toml
|   `-- ports/
|       |-- goldeneye-ports/
|       |   |-- src/
|       |   |   |-- adr_traces.rs
|       |   |   |-- artifact.rs
|       |   |   |-- crosslink.rs
|       |   |   |-- discovery.rs
|       |   |   |-- edit_syntax.rs
|       |   |   |-- edit.rs
|       |   |   |-- error.rs
|       |   |   |-- git_history.rs
|       |   |   |-- git.rs
|       |   |   |-- index_syntax.rs
|       |   |   |-- index.rs
|       |   |   |-- inspection.rs
|       |   |   |-- lib.rs
|       |   |   |-- project_administration.rs
|       |   |   |-- query.rs
|       |   |   |-- repository.rs
|       |   |   `-- semantic_index.rs
|       |   `-- Cargo.toml
|       `-- README.md
|-- docs/
|   |-- benchmarks/
|   |   |-- 2026-07-14-codebase-memory-vs-goldeneye.md
|   |   `-- PERFORMANCE-OPTIMIZATION-PROMPT.md
|   |-- superfastpowers/
|   |   |-- plans/
|   |   |   |-- GD/
|   |   |   |   |-- 2026-07-13-goldeneye-discovery/
|   |   |   |   |   |-- tasks/
|   |   |   |   |   |   |-- GD-1/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GD-2/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GD-3/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GD-4/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GD-5/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GD-6/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   `-- GD-7/
|   |   |   |   |   |       |-- code-quality.md
|   |   |   |   |   |       |-- context.md
|   |   |   |   |   |       |-- implementer-handoff.md
|   |   |   |   |   |       |-- spec-review.md
|   |   |   |   |   |       `-- task.md
|   |   |   |   |   |-- final-review.md
|   |   |   |   |   `-- plan-progression.md
|   |   |   |   `-- 2026-07-13-goldeneye-discovery.md
|   |   |   |-- GF/
|   |   |   |   |-- 2026-07-13-goldeneye-foundation/
|   |   |   |   |   |-- tasks/
|   |   |   |   |   |   |-- GF-1/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-2/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-3/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-4/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-5/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-6/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GF-7/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   `-- GF-8/
|   |   |   |   |   |       |-- code-quality.md
|   |   |   |   |   |       |-- context.md
|   |   |   |   |   |       |-- spec-review.md
|   |   |   |   |   |       `-- task.md
|   |   |   |   |   |-- final-review.md
|   |   |   |   |   `-- plan-progression.md
|   |   |   |   `-- 2026-07-13-goldeneye-foundation.md
|   |   |   |-- GFP/
|   |   |   |   |-- 2026-07-13-goldeneye-full-grammar-provider/
|   |   |   |   |   |-- tasks/
|   |   |   |   |   |   |-- GFP-1/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GFP-2/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GFP-3/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   |-- GFP-4/
|   |   |   |   |   |   |   |-- code-quality.md
|   |   |   |   |   |   |   |-- context.md
|   |   |   |   |   |   |   |-- implementer-handoff.md
|   |   |   |   |   |   |   |-- spec-review.md
|   |   |   |   |   |   |   `-- task.md
|   |   |   |   |   |   `-- GFP-5/
|   |   |   |   |   |       |-- code-quality.md
|   |   |   |   |   |       |-- context.md
|   |   |   |   |   |       |-- implementer-handoff.md
|   |   |   |   |   |       |-- spec-review.md
|   |   |   |   |   |       `-- task.md
|   |   |   |   |   `-- plan-progression.md
|   |   |   |   `-- 2026-07-13-goldeneye-full-grammar-provider.md
|   |   |   `-- GS/
|   |   |       |-- 2026-07-13-goldeneye-syntax-core/
|   |   |       |   |-- tasks/
|   |   |       |   |   |-- GS-1/
|   |   |       |   |   |   |-- code-quality.md
|   |   |       |   |   |   |-- context.md
|   |   |       |   |   |   |-- implementer-handoff.md
|   |   |       |   |   |   |-- spec-review.md
|   |   |       |   |   |   `-- task.md
|   |   |       |   |   |-- GS-2/
|   |   |       |   |   |   |-- code-quality.md
|   |   |       |   |   |   |-- context.md
|   |   |       |   |   |   |-- implementer-handoff.md
|   |   |       |   |   |   |-- spec-review.md
|   |   |       |   |   |   `-- task.md
|   |   |       |   |   |-- GS-3/
|   |   |       |   |   |   |-- code-quality.md
|   |   |       |   |   |   |-- context.md
|   |   |       |   |   |   |-- spec-review.md
|   |   |       |   |   |   `-- task.md
|   |   |       |   |   |-- GS-4/
|   |   |       |   |   |   |-- code-quality.md
|   |   |       |   |   |   |-- context.md
|   |   |       |   |   |   |-- spec-review.md
|   |   |       |   |   |   `-- task.md
|   |   |       |   |   `-- GS-5/
|   |   |       |   |       |-- code-quality.md
|   |   |       |   |       |-- context.md
|   |   |       |   |       |-- implementer-handoff.md
|   |   |       |   |       |-- spec-review.md
|   |   |       |   |       `-- task.md
|   |   |       |   |-- final-review.md
|   |   |       |   `-- plan-progression.md
|   |   |       `-- 2026-07-13-goldeneye-syntax-core.md
|   |   `-- specs/
|   |       |-- 2026-07-13-goldeneye-full-grammar-provider-design.md
|   |       `-- 2026-07-13-goldeneye-rust-port-design.md
|   |-- gcal-acceptance.md
|   |-- gcal-feature-handoff.md
|   |-- architecture.md
|   `-- full-grammar-pack.md
|-- grammars/
|   |-- full-pack-license-ledger.md
|   `-- full-pack.toml
|-- packaging/
|   |-- tests/
|   |   `-- test_packaging.py
|   |-- README.md
|   |-- render_release.py
|   `-- verify_release.py
|-- pkg/
|   |-- chocolatey/
|   |   |-- tools/
|   |   |   |-- chocolateyinstall.ps1.tmpl
|   |   |   `-- chocolateyuninstall.ps1
|   |   |-- goldeneye-tool.nuspec.tmpl
|   |   |-- LICENSE
|   |   |-- NOTICE
|   |   `-- README.md
|   |-- go/
|   |   |-- cmd/
|   |   |   `-- goldeneye/
|   |   |       |-- main_test.go
|   |   |       `-- main.go
|   |   `-- README.md
|   |-- homebrew/
|   |   |-- Formula/
|   |   |   `-- goldeneye-tool.rb.tmpl
|   |   `-- README.md
|   |-- nix/
|   |   |-- flake.nix.tmpl
|   |   |-- goldeneye-tool-bin.nix.tmpl
|   |   `-- README.md
|   |-- npm/
|   |   |-- test/
|   |   |   `-- install.test.js
|   |   |-- bin.js
|   |   |-- install.js
|   |   |-- LICENSE
|   |   |-- NOTICE
|   |   |-- package.json
|   |   `-- README.md
|   `-- pypi/
|       |-- src/
|       |   `-- goldeneye_tool/
|       |       |-- __init__.py
|       |       |-- __main__.py
|       |       `-- _cli.py
|       |-- tests/
|       |   `-- test_cli.py
|       |-- LICENSE
|       |-- NOTICE
|       |-- pyproject.toml
|       `-- README.md
|-- tests/
|   `-- fixtures/
|       |-- gcal/
|       |   |-- rust-project/
|       |   |   |-- src/
|       |   |   |   |-- billing.rs
|       |   |   |   |-- lib.rs
|       |   |   |   `-- shipping.rs
|       |   |   `-- Cargo.toml
|       |   `-- expected.json
|       |-- edit/
|       |   |-- rust-project/
|       |   |   `-- src/
|       |   |       `-- lib.rs
|       |   `-- expected-tools.json
|       `-- mcp/
|           |-- foundation.expected.jsonl
|           `-- foundation.jsonl
|-- tools/
|   |-- gcal-acceptance.mjs
|   |-- gcal-acceptance.ps1
|   |-- benchmark-competitors.mjs
|   |-- edit-acceptance.mjs
|   |-- edit-acceptance.ps1
|   |-- export_grammar_lock.py
|   |-- export_upstream_languages.py
|   |-- generate-index-language-corpus.mjs
|   |-- generate-index-language-specs.mjs
|   `-- test_export_grammar_lock.py
|-- ui/
|   |-- @/
|   |   `-- components/
|   |       `-- ui/
|   |           |-- badge.tsx
|   |           |-- button.tsx
|   |           |-- card.tsx
|   |           |-- checkbox.tsx
|   |           |-- input.tsx
|   |           |-- scroll-area.tsx
|   |           `-- separator.tsx
|   |-- public/
|   |   `-- runtime-config.js
|   |-- scripts/
|   |   |-- asset-manifest.mjs
|   |   |-- capture-upstream.mjs
|   |   |-- dist-manifest.mjs
|   |   |-- gate.mjs
|   |   |-- lib-assets.mjs
|   |   `-- license-assets.mjs
|   |-- src/
|   |   |-- api/
|   |   |   |-- basePath.ts
|   |   |   |-- contract.ts
|   |   |   `-- rpc.ts
|   |   |-- components/
|   |   |   |-- ui/
|   |   |   |   |-- badge.tsx
|   |   |   |   |-- button.tsx
|   |   |   |   |-- card.tsx
|   |   |   |   |-- checkbox.tsx
|   |   |   |   |-- input.tsx
|   |   |   |   |-- scroll-area.tsx
|   |   |   |   `-- separator.tsx
|   |   |   |-- ControlTab.tsx
|   |   |   |-- DisplaySettingsMenu.tsx
|   |   |   |-- EdgeLines.tsx
|   |   |   |-- ErrorBoundary.tsx
|   |   |   |-- FilterPanel.tsx
|   |   |   |-- GraphLoader.tsx
|   |   |   |-- GraphScene.test.ts
|   |   |   |-- GraphScene.tsx
|   |   |   |-- GraphTab.deadcode.test.tsx
|   |   |   |-- GraphTab.filters.test.tsx
|   |   |   |-- GraphTab.test.ts
|   |   |   |-- GraphTab.tsx
|   |   |   |-- MissedCallout.tsx
|   |   |   |-- NodeCloud.tsx
|   |   |   |-- NodeDetailPanel.test.tsx
|   |   |   |-- NodeDetailPanel.tsx
|   |   |   |-- NodeLabels.tsx
|   |   |   |-- NodeTooltip.tsx
|   |   |   |-- ProjectCard.tsx
|   |   |   |-- ResizeHandle.tsx
|   |   |   |-- Sidebar.tsx
|   |   |   |-- StatsTab.test.tsx
|   |   |   |-- StatsTab.tsx
|   |   |   `-- TabBar.tsx
|   |   |-- contracts/
|   |   |   |-- api.node.test.ts
|   |   |   |-- assets.node.test.ts
|   |   |   |-- escaping.node.test.tsx
|   |   |   `-- layout.node.test.ts
|   |   |-- fixtures/
|   |   |   `-- cameraLayout.ts
|   |   |-- hooks/
|   |   |   |-- useGraphData.test.ts
|   |   |   |-- useGraphData.ts
|   |   |   `-- useProjects.ts
|   |   |-- lib/
|   |   |   |-- cameraLayout.ts
|   |   |   |-- colors.ts
|   |   |   |-- density.test.ts
|   |   |   |-- density.ts
|   |   |   |-- i18n.test.ts
|   |   |   |-- i18n.ts
|   |   |   |-- types.ts
|   |   |   `-- utils.ts
|   |   |-- styles/
|   |   |   `-- globals.css
|   |   |-- App.tsx
|   |   |-- main.tsx
|   |   `-- vite-env.d.ts
|   |-- .gitignore
|   |-- asset-manifest.json
|   |-- checksums.sha256
|   |-- components.json
|   |-- HTTP_API_CONTRACT.md
|   |-- index.html
|   |-- LICENSE
|   |-- license-policy.json
|   |-- package-lock.json
|   |-- package.json
|   |-- PORT_PROVENANCE.json
|   |-- README.md
|   |-- THIRD_PARTY_LICENSES.md
|   |-- tsconfig.json
|   |-- UPSTREAM_LICENSE
|   |-- UPSTREAM_SOURCES.sha256
|   `-- vite.config.ts
|-- xtask/
|   |-- src/
|   |   |-- architecture.rs
|   |   |-- lib.rs
|   |   `-- main.rs
|   |-- tests/
|   |   |-- full_pack_ci.rs
|   |   |-- grammar_sync.rs
|   |   `-- provider_generation.rs
|   `-- Cargo.toml
|-- .cbmignore
|-- .gitattributes
|-- .gitignore
|-- Cargo.lock
|-- Cargo.toml
|-- flake.nix
|-- go.mod
|-- LICENSE
|-- NOTICE
|-- rust-toolchain.toml
|-- rustfmt.toml
`-- THIRD_PARTY.md
```

