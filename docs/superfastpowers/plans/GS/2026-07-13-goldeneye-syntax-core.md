# Goldeneye Syntax Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:subagent-driven-development (recommended), superfastpowers:goal-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Tree-sitter syntax core with reusable grammar providers, parser snapshots, diagnostics, stable named-node locators, and bounded token-efficient inspection output.

**Architecture:** `goldeneye-domain` owns tool-neutral language/syntax identities; discovery re-exports the shared `LanguageId`. `goldeneye-syntax` depends on domain IDs and a `GrammarProvider` boundary, never on discovery traversal/ignore code. A six-language core provider supports local development; thread-local parsers produce immutable snapshots over raw bytes. Locators combine snapshot guards, named-child ancestry, byte spans, and BLAKE3 hashes; compact inspection never exposes raw whole-file source by default.
**Plan Acronym:** GS


**Tech Stack:** Rust 1.97.0, `tree-sitter 0.26.11`, core grammar crates for Rust/Python/JavaScript/TypeScript/TSX/Go, `blake3`, `serde`, standard `Arc` and thread-local parser reuse.

---

## File Structure

- `crates/goldeneye-syntax/Cargo.toml`: syntax/runtime/core grammar dependencies.
- `crates/goldeneye-syntax/src/grammar.rs`: provider trait, grammar metadata, core provider.
- `crates/goldeneye-syntax/src/engine.rs`: parser reuse, snapshots, diagnostics, incremental reparse.
- `crates/goldeneye-syntax/src/locator.rs`: hashes, locator construction, exact resolution, stale errors.
- `crates/goldeneye-syntax/src/inspect.rs`: bounded flat named-node views.
- `crates/goldeneye-syntax/src/pack.rs`: full-pack lock schema, validation, and provenance queries.
- `crates/goldeneye-syntax/src/lib.rs`: public API.
- `crates/goldeneye-domain/src/syntax.rs`: shared hashes, generations, spans, normalized path, grammar/file identities, and locator scope/anchors; domain root also owns `LanguageId`.
- `crates/goldeneye-syntax/tests/core_grammars.rs`: six-language provider/parse tests.
- `crates/goldeneye-syntax/tests/diagnostics.rs`: malformed/byte/error/incremental behavior.
- `crates/goldeneye-syntax/tests/locators.rs`: all named nodes, resolution, stale guards.
- `crates/goldeneye-syntax/tests/inspect.rs`: depth/node/text bounds and deterministic output.
- `grammars/full-pack.toml`: 159 grammar assets plus 160 explicit language bindings with ABI/provenance/hash/availability metadata.
- `tools/export_grammar_lock.py`: deterministic lock exporter from pinned upstream manifest/assets.
- `.cargo/config.toml`: workspace-local `cargo xtask` alias.
- `xtask/src/main.rs`: explicit offline grammar materialization command.
- `THIRD_PARTY.md`: Tree-sitter runtime/core grammar licenses and full-pack provenance.

### Task 1: Create Grammar Provider and Six-Language Core

<TASK-ID>GS-1</TASK-ID>

**Files:**
- Create: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/src/lib.rs`
- Create: `crates/goldeneye-syntax/src/grammar.rs`
- Create: `crates/goldeneye-syntax/tests/core_grammars.rs`
- Create: `crates/goldeneye-domain/tests/language_id.rs`
- Create: `crates/goldeneye-discovery/tests/domain_ids.rs`
- Modify: `crates/goldeneye-domain/src/lib.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Modify: `crates/goldeneye-discovery/Cargo.toml`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Create syntax crate manifest**

```toml
[package]
name = "goldeneye-syntax"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
goldeneye-domain = { path = "../goldeneye-domain" }
serde.workspace = true
thiserror.workspace = true
tree-sitter = "=0.26.11"
tree-sitter-go = "=0.25.0"
tree-sitter-javascript = "=0.25.0"
tree-sitter-python = "=0.25.0"
tree-sitter-rust = "=0.24.2"
tree-sitter-typescript = "=0.23.2"

[lints]
workspace = true
```

- [ ] **Step 2: Write failing shared-language-ID tests**

Move the existing `LanguageId` contract into `goldeneye-domain`. Test non-empty preservation, empty rejection, ordering/hash behavior, and type identity through `goldeneye_discovery::LanguageId`. Discovery must use `pub use goldeneye_domain::LanguageId`; it must not define a wrapper or conversion copy.

- [ ] **Step 3: Run shared-ID tests and verify RED**

Run: `cargo test -p goldeneye-domain --test language_id && cargo test -p goldeneye-discovery --test domain_ids`

Expected: FAIL because domain does not own `LanguageId` and discovery does not re-export it.

- [ ] **Step 4: Migrate the ID and make the constructor error change explicit**

Add `LanguageId` and its typed validation error to domain, add the domain dependency to discovery, delete discovery's duplicate definition, and publicly re-export the domain type from discovery. This intentionally changes `LanguageId::new("")` from `DiscoveryError::InvalidLanguageId` to the domain validation error; update the existing discovery unit assertion and document the 0.1 pre-release API change. Callers using valid IDs retain exact type identity. Run the entire discovery suite to prove registry/walker behavior remains unchanged.

- [ ] **Step 5: Write failing provider tests**

```rust
#[test]
fn core_provider_exposes_exact_language_set() {
    let provider = CoreGrammarProvider;
    assert_eq!(
        provider
            .supported_ids()
            .iter()
            .map(LanguageId::as_str)
            .collect::<Vec<_>>(),
        ["go", "javascript", "python", "rust", "tsx", "typescript"]
    );
}

#[test]
fn every_core_grammar_parses_valid_source() {
    for (id, source, root_kind) in fixtures() {
        let grammar = CoreGrammarProvider.grammar(&LanguageId::new(id).unwrap()).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&grammar.language).unwrap();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        assert_eq!(tree.root_node().kind(), root_kind);
        assert!(!tree.root_node().has_error());
        let abi = usize::try_from(grammar.abi).unwrap();
        assert!((tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION
            ..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi));
        assert!(grammar.language.node_kind_count() > 0);
    }
}

#[test]
fn provider_reports_pinned_metadata_and_typed_unsupported_error() {
    for (id, package, version) in expected_core_metadata() {
        let grammar = CoreGrammarProvider.grammar(&LanguageId::new(id).unwrap()).unwrap();
        assert_eq!(grammar.language_id.as_str(), id);
        assert_eq!(
            grammar.source,
            GrammarSource::RustCrate { package: package.into(), version: version.into() }
        );
        assert_eq!(usize::try_from(grammar.abi).unwrap(), grammar.language.abi_version());
    }
    assert!(matches!(
        CoreGrammarProvider.grammar(&LanguageId::new("java").unwrap()),
        Err(SyntaxError::UnsupportedGrammar { .. })
    ));
}
```

Fixtures contain one small valid declaration for each of Go, JavaScript, Python, Rust, TypeScript, and TSX. Metadata fixtures pin the exact crate package/version declared in the manifest.

- [ ] **Step 6: Run provider tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test core_grammars`

Expected: FAIL because provider/crate does not exist.

- [ ] **Step 7: Implement provider boundary**

```rust
pub struct Grammar {
    pub language_id: LanguageId,
    pub language: tree_sitter::Language,
    pub abi: u32,
    pub source: GrammarSource,
}

pub enum GrammarSource {
    RustCrate { package: String, version: String },
    FullPack { grammar: String, source_hash: String },
}

pub trait GrammarProvider: Send + Sync {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError>;
    fn supported_ids(&self) -> Vec<LanguageId>;
}
```

`Grammar`/`GrammarSource` derive the owned debug/clone/equality traits needed by snapshots and metadata tests; no metadata field borrows provider-local storage.

`CoreGrammarProvider` maps exact IDs to grammar constants:

- `rust → tree_sitter_rust::LANGUAGE`
- `python → tree_sitter_python::LANGUAGE`
- `javascript → tree_sitter_javascript::LANGUAGE`
- `typescript → tree_sitter_typescript::LANGUAGE_TYPESCRIPT`
- `tsx → tree_sitter_typescript::LANGUAGE_TSX`
- `go → tree_sitter_go::LANGUAGE`

Convert each `LanguageFn` through `.into()` and record the generated grammar ABI from `Language::abi_version` through checked `u32` conversion. Do not make the provider contract depend on grammar-crate-only `NODE_TYPES` JSON: the pinned full-pack assets expose node/field metadata through the compiled `Language` API but do not carry those JSON files. Return typed `UnsupportedGrammar` for other IDs. Sort `supported_ids` lexically.

- [ ] **Step 8: Verify provider**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

Expected: shared ID migration, all discovery regression tests, provider tests, and six grammar parse tests pass.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/goldeneye-domain crates/goldeneye-discovery crates/goldeneye-syntax
git commit -m "[GS-1] feat: add Tree-sitter grammar provider"
```

### Task 2: Implement Parser Snapshots and Diagnostics

<TASK-ID>GS-2</TASK-ID>

**Files:**
- Create: `crates/goldeneye-domain/src/syntax.rs`
- Create: `crates/goldeneye-domain/tests/syntax_types.rs`
- Modify: `crates/goldeneye-domain/src/lib.rs`
- Modify: `crates/goldeneye-domain/Cargo.toml`
- Create: `crates/goldeneye-syntax/src/engine.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/tests/diagnostics.rs`

- [ ] **Step 1: Write failing tool-neutral syntax identity tests**

Add domain tests for:

- `ContentHash::of` BLAKE3 output, lowercase 64-character display/parse, and JSON string round-trip rather than a 32-integer array;
- typed `Generation`, `ByteSpan`, and `SourcePoint { row, column_bytes }` (`row` and byte-column are zero-based);
- `ProjectRelativePath` accepting normalized Unicode slash paths and rejecting absolute, drive-prefixed, backslash, empty-segment, `.`, `..`, NUL, and empty paths;
- `GrammarFingerprint`, `LocatorScope`, `AncestorStep`, `NodeAnchor`, and `NodeLocator` exact JSON golden round-trip.

All serialized offsets use `u64`; syntax converts to/from Tree-sitter `usize` with checked conversions.

- [ ] **Step 2: Run domain tests and verify RED**

Run: `cargo test -p goldeneye-domain --test syntax_types`

Expected: FAIL because shared syntax identities do not exist.

- [ ] **Step 3: Implement shared syntax identities in domain**

```rust
pub struct Generation(u64);
pub struct ContentHash([u8; 32]);
pub struct ByteSpan { pub start: u64, pub end: u64 }
pub struct SourcePoint { pub row: u64, pub column_bytes: u64 }
pub struct SourceSpan { pub bytes: ByteSpan, pub start: SourcePoint, pub end: SourcePoint }
pub struct ProjectRelativePath(String);
pub struct GrammarFingerprint {
    pub provider: String,
    pub grammar: String,
    pub revision: String,
    pub abi: u32,
}
pub struct FileContext {
    pub project_id: ProjectId,
    pub relative_path: ProjectRelativePath,
}
pub struct LocatorScope {
    pub file: FileContext,
    pub language_id: LanguageId,
    pub grammar: GrammarFingerprint,
    pub file_hash: ContentHash,
    pub generation: Generation,
}
pub struct AncestorStep {
    pub node_kind: String,
    pub named_child_index: u32,
    pub field_name: Option<String>,
}
pub struct NodeAnchor {
    pub ancestor_path: Vec<AncestorStep>,
    pub node_kind: String,
    pub source_span: SourceSpan,
    pub content_hash: ContentHash,
}
pub struct NodeLocator { pub scope: LocatorScope, pub anchor: NodeAnchor }
```

These types derive/implement owned equality, hashing where meaningful, and compact validated Serde without importing Tree-sitter or filesystem APIs. Deserialization must invoke constructors/invariant checks, never bypass them through transparent unchecked derives. Add compatible validated Serde to existing `ProjectId` and `LanguageId`, add `serde` and `blake3` plus test-only `serde_json` to `goldeneye-domain`, and re-export the syntax types from its root.

- [ ] **Step 4: Write failing snapshot/raw-byte/diagnostic tests**

```rust
#[test]
fn snapshot_preserves_raw_bytes_tree_hash_and_generation() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let source = Arc::<[u8]>::from(&b"fn main() { let raw = \xff; }"[..]);
    let snapshot = engine.parse(
        LanguageId::new("rust").unwrap(),
        source.clone(),
        Generation::new(7),
    ).unwrap();
    assert_eq!(snapshot.source(), source.as_ref());
    assert_eq!(snapshot.generation(), Generation::new(7));
    assert_eq!(snapshot.root().kind(), "source_file");
    assert_eq!(snapshot.file_hash(), ContentHash::of(&source));
    let abi = usize::try_from(snapshot.grammar().abi).unwrap();
    assert!((tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION
        ..=tree_sitter::LANGUAGE_VERSION)
        .contains(&abi));
}

#[test]
fn malformed_source_returns_snapshot_with_bounded_diagnostics() {
    let engine = SyntaxEngine::new(CoreGrammarProvider);
    let snapshot = engine.parse(
        LanguageId::new("python").unwrap(),
        many_independent_python_errors(200),
        Generation::new(1),
    ).unwrap();
    assert!(snapshot.has_errors());
    assert!(snapshot.diagnostic_total() > MAX_DIAGNOSTIC_DETAILS);
    assert_eq!(snapshot.diagnostics().len(), MAX_DIAGNOSTIC_DETAILS);
    assert!(snapshot.diagnostics_truncated());
    assert!(snapshot.diagnostics().windows(2).all(in_source_order));
}
```

- [ ] **Step 5: Run engine tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test diagnostics`

Expected: FAIL because engine/snapshot types are undefined.

- [ ] **Step 6: Implement parser reuse and immutable snapshots**

Use one thread-local `HashMap<String, tree_sitter::Parser>`. For every parse:

1. obtain grammar from provider;
2. get/create parser by language ID;
3. set exact language;
4. parse raw `Arc<[u8]>`;
5. return typed `ParseCancelled` if Tree-sitter returns `None`;
6. traverse tree once for diagnostics;
7. store immutable source/tree/hash/generation/language/grammar-fingerprint metadata.

`SyntaxSnapshot` owns `tree_sitter::Tree` and `Arc<[u8]>`. It exposes borrowed root/source and scalar metadata; it never exposes mutable tree access. Project/path are intentionally external. Syntax later combines a current `FileContext` with snapshot guards into `LocatorScope`, so a resolver can verify every scope field.

Grammar fingerprints are canonical and tested: core uses provider `rust-crate`, grammar language ID, revision `<package>@<exact-version>`, and ABI; full-pack sources later use provider `full-pack`, locked grammar asset name, revision source hash, and ABI.

- [ ] **Step 7: Implement diagnostic traversal**

```rust
pub const MAX_DIAGNOSTIC_DETAILS: usize = 128;

pub enum DiagnosticKind {
    Error,
    Missing,
}

pub struct SyntaxDiagnostic {
    pub kind: DiagnosticKind,
    pub node_kind: String,
    pub span: SourceSpan,
}
```

Traverse iteratively with `TreeCursor`. Count every error/missing node, retain exactly the first 128 in deterministic source/preorder, and expose total/retained/truncated independently. Add focused assertions for an error node, a missing node, zero-width missing span, multibyte byte columns, exact cap, total count, and retained ordering.

- [ ] **Step 8: Add validated incremental reparse tests and implementation**

Test insertion inside the multibyte Rust identifier in `fn café() {}`. The edit point must use byte column 8, not character column 7. Assert:

- new source/hash/generation;
- unchanged root kind;
- no errors;
- changed range reported by `Tree::changed_ranges` is bounded around edit.

`SyntaxEngine::reparse` validates old/new byte bounds, byte deltas, and zero-based byte points against old/new raw source; clones the previous tree; applies `InputEdit`; and passes the edited tree to the parser with new bytes. It returns `ReparseResult { snapshot, changed_ranges: Vec<SourceSpan> }`, computed from the edited old tree against the new tree without mutating the prior snapshot. It rejects inconsistent edits, language changes, and non-increasing generations with typed errors.

- [ ] **Step 9: Verify engine**

Run: `cargo test -p goldeneye-domain --test syntax_types && cargo test -p goldeneye-syntax --test diagnostics && cargo clippy --workspace --all-targets -- -D warnings`

Expected: domain identity/Serde, valid, malformed, invalid-UTF-8 raw-byte, diagnostic-cap/order, and multibyte incremental tests pass.

- [ ] **Step 10: Commit**

```bash
git add Cargo.lock crates/goldeneye-domain crates/goldeneye-syntax
git commit -m "[GS-2] feat: parse immutable syntax snapshots"
```

### Task 3: Implement Stable Named-Node Locators

<TASK-ID>GS-3</TASK-ID>

**Files:**
- Create: `crates/goldeneye-syntax/src/locator.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Create: `crates/goldeneye-syntax/tests/locators.rs`

- [ ] **Step 1: Write failing locator coverage tests**

```rust
#[test]
fn every_named_node_has_unique_resolvable_locator() {
    let snapshot = rust_snapshot("fn alpha(x: i32) -> i32 { x + 1 }");
    let context = file_context("project", "src/lib.rs");
    let locators = all_named_locators(&snapshot, &context).unwrap();
    assert!(!locators.is_empty());
    let unique: HashSet<_> = locators.iter().collect();
    assert_eq!(unique.len(), locators.len());
    for locator in &locators {
        let node = resolve_locator(&snapshot, &context, locator).unwrap();
        assert_eq!(node.kind(), locator.anchor.node_kind.as_str());
        assert_eq!(node.start_byte() as u64, locator.anchor.source_span.bytes.start);
        assert_eq!(node.end_byte() as u64, locator.anchor.source_span.bytes.end);
    }
}
```

- [ ] **Step 2: Write failing stale-guard tests**

Build a valid locator, mutate one guard at a time, and cover independently:

- wrong project ID;
- wrong project-relative file;
- wrong language ID;
- wrong grammar provider/name/revision/ABI;
- wrong file hash;
- wrong generation;
- invalid ancestor named index;
- ancestor kind mismatch;
- ancestor field-name mismatch;
- terminal kind mismatch;
- terminal byte range mismatch;
- terminal point span mismatch;
- terminal content hash mismatch.

Every case returns a distinct typed `LocatorError`, cannot return a node, and never falls back to byte-only or fuzzy matching. Add a locator JSON golden/round-trip test here as an integration check over real source-derived values.

- [ ] **Step 3: Run tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test locators`

Expected: FAIL because locator API is undefined.

- [ ] **Step 4: Implement scope construction and typed errors**

`locator_scope(snapshot, file_context)` derives language, grammar fingerprint, file hash, and generation only from the immutable snapshot; callers cannot override them. `resolve_locator(snapshot, current_file_context, locator)` reconstructs the actual scope the same way and compares every field before traversing. Errors identify the exact failed guard but never include raw source bytes.

- [ ] **Step 5: Implement iterative locator construction/resolution**

Build ancestor steps from root through named children. Each step records parent-relative named-child index, expected child kind, and field name when Tree-sitter exposes one. When resolving, recover the raw child index while iterating named children before checking Tree-sitter's field name; a named index is not a raw child index. Root locator uses an empty ancestor path.

Resolution order is exactly current project/path -> snapshot language/grammar/hash/generation -> ancestor index/kind/field traversal -> terminal kind/byte+point span -> content hash. It returns `tree_sitter::Node` borrowed from snapshot only after every check passes.

- [ ] **Step 6: Verify locators**

Run: `cargo test -p goldeneye-syntax --test locators`

Expected: all named nodes are unique/resolvable; domain JSON is portable; every scope, ancestor, terminal, and stale mutation rejects with its exact error.

- [ ] **Step 7: Commit**

```bash
git add crates/goldeneye-syntax
git commit -m "[GS-3] feat: add guarded syntax node locators"
```

### Task 4: Add Bounded Token-Efficient Syntax Inspection

<TASK-ID>GS-4</TASK-ID>

**Files:**
- Create: `crates/goldeneye-syntax/src/inspect.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/tests/inspect.rs`
- Create: `crates/goldeneye-syntax/tests/fixtures/compact-inspection.json`

- [ ] **Step 1: Write failing inspection tests**

```rust
#[test]
fn inspection_is_deterministic_named_only_and_resolvable() {
    let snapshot = rust_snapshot("struct A { x: i32 }\nfn f() {}");
    let request = InspectRequest::default();
    let first = inspect_syntax(&snapshot, &context(), &request).unwrap();
    let second = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert_eq!(first, second);
    for view in &first.nodes {
        let locator = first.locator(view.ordinal).unwrap();
        resolve_locator(&snapshot, &context(), &locator).unwrap();
    }
}

#[test]
fn inspection_enforces_depth_node_and_preview_bounds() {
    let snapshot = deeply_nested_snapshot();
    let request = InspectRequest {
        max_depth: 2,
        max_nodes: 5,
        preview_chars: 8,
        byte_range: None,
    };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert!(view.nodes.len() <= 5);
    assert!(view.nodes.iter().all(|node| node.depth <= 2));
    assert!(view.nodes.iter().all(|node| node
        .preview
        .as_ref()
        .is_none_or(|preview| preview.chars().count() <= 8)));
    assert!(view.truncated);
    assert!(view.total_named_nodes_seen >= view.nodes.len());
}

#[test]
fn non_root_range_keeps_one_base_path_and_resolvable_deltas() {
    let snapshot = rust_snapshot("fn outer() { if true { let answer = 42; } }");
    let range = byte_span_of(&snapshot, "let_declaration");
    let request = InspectRequest { byte_range: Some(range), ..InspectRequest::default() };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert!(!view.base_ancestor_path.is_empty());
    for node in &view.nodes {
        let locator = view.locator(node.ordinal).unwrap();
        resolve_locator(&snapshot, &context(), &locator).unwrap();
    }
}

#[test]
fn invalid_range_and_over_cap_requests_are_typed_errors() {
    let snapshot = rust_snapshot("fn f() {}");
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &request_with_end(snapshot.source().len() as u64 + 1)),
        Err(InspectError::RangeOutOfBounds { .. })
    ));
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &request_with_max_depth(33)),
        Err(InspectError::LimitExceeded { field: "max_depth", .. })
    ));
}

#[test]
fn compact_serialization_has_stable_shape_and_budget() {
    let inspection = inspect_syntax(&wide_200_node_snapshot(), &context(), &InspectRequest::default()).unwrap();
    let encoded = serde_json::to_vec(&inspection).unwrap();
    assert!(encoded.len() <= 32_768, "{} bytes", encoded.len());
    let small = inspect_syntax(&rust_snapshot("fn f() {}"), &context(), &InspectRequest::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&small).unwrap(),
        include_str!("fixtures/compact-inspection.json").trim()
    );
}
```

Add focused preview cases over raw bytes for `é`, an emoji, newline, backslash, and `0xff`. Escaping happens as indivisible atoms: a cap may omit `\\n` or `\\\\`, but can never return half an escape. Assert no raw newline, valid UTF-8 output, replacement character only for invalid source bytes, and scalar count at or below the requested cap.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test inspect`

Expected: FAIL because inspection API is undefined.

- [ ] **Step 3: Implement bounded compact request/result types**

Defaults and hard caps:

- `max_depth = 4`, hard cap `32`;
- `max_nodes = 200`, hard cap `1000`;
- `preview_chars = 0` by default, hard cap `256` Unicode scalar values;
- optional domain `ByteSpan` must be ordered and lie inside source.

Reject values beyond hard caps with typed errors rather than silently clamping.

`SyntaxInspection` contains one shared `LocatorScope`, one base ancestor path for a ranged subtree, and flat preorder nodes. A node contains only ordinal, parent ordinal, depth, parent-relative named-child index/field, kind, byte/point span, content hash, named-child count, and optional preview. It never repeats scope or full ancestor prefixes. `SyntaxInspection::locator(ordinal)` reconstructs a full domain `NodeLocator` by following parent ordinals plus the shared base path. Serde uses documented compact field names; the golden test makes wire drift explicit.

Add test-only `serde_json` to `goldeneye-syntax` for the golden and encoded-size gate.

Preview decoding is lossy UTF-8, then escaped into indivisible single-line atoms and bounded by Unicode scalar values without splitting an atom. Hashes/spans always use original bytes.

- [ ] **Step 4: Implement iterative preorder inspection**

Traverse named nodes only; prune outside optional byte range and beyond depth. For a ranged request, retain one shared base path sufficient to reconstruct every emitted locator. Validate parent ordinals form an acyclic earlier-node chain. Count nodes seen even after result cap so `truncated` and total remain truthful. Never recurse on call stack.

- [ ] **Step 5: Verify inspection**

Run: `cargo fmt --check && cargo clippy -p goldeneye-syntax --all-targets -- -D warnings && cargo test -p goldeneye-syntax`

Expected: deterministic, bounded, range-filtered, multibyte/escape/invalid-byte preview, compact JSON golden/budget, and reconstructed-locator resolution tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-syntax
git commit -m "[GS-4] feat: inspect syntax with bounded context"
```

### Task 5: Freeze Full Grammar-Pack Metadata and Offline Sync

<TASK-ID>GS-5</TASK-ID>

This is an intermediate metadata/materialization slice. It does **not** claim 160-language runtime completion: release compilation, generated `FullGrammarProvider`, every-grammar parse probes, the full-pack CI job, and release embedding belong to the required successor phase **GFP — Full Grammar Provider Runtime**. Until GFP passes, only the six core grammars are executable and release builds are not full-pack completion evidence.

**Files:**
- Create: `grammars/full-pack.toml`
- Create: `tools/export_grammar_lock.py`
- Create: `.cargo/config.toml`
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`
- Create: `xtask/tests/grammar_sync.rs`
- Create: `crates/goldeneye-syntax/src/pack.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/tests/grammar_lock.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `THIRD_PARTY.md`

- [ ] **Step 1: Write failing lock completeness test**

```rust
#[test]
fn full_pack_lock_matches_audited_upstream() {
    let lock = GrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap();
    assert_eq!(lock.upstream_commit(), "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c");
    assert_eq!(lock.grammars.len(), 159);
    assert_eq!(lock.language_mappings.len(), 160);
    assert_eq!(lock.abi_histogram(), BTreeMap::from([(13, 9), (14, 78), (15, 72)]));
    assert_eq!(lock.available_language_count(), 159);
    assert_eq!(lock.unique_bound_grammar_count(), 157);
    assert_eq!(lock.unavailable_language_ids(), ["nim"]);
    assert_eq!(
        lock.orphan_grammar_names(),
        ["objectscript_routine", "objectscript_udl"]
    );
    assert_eq!(lock.grammar_name_for("yaml").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("kustomize").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("k8s").unwrap(), "yaml");
    assert!(lock.grammars.iter().all(|g| !g.source_hash.is_empty()));
    assert!(lock.grammars.iter().all(|g| !g.license_files.is_empty()));
}
```

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-syntax --test grammar_lock`

Expected: FAIL because lock/export types do not exist.

- [ ] **Step 3: Implement lock schema, validation, and deterministic exporter**

`pack.rs` deserializes the TOML into owned records. Top-level metadata declares grammar count, language-binding count, compatible ABI range, and upstream commit; validation checks those declared counts plus unique names/IDs, relative slash-normalized paths, ABI compatibility, non-empty hashes, and non-empty license declarations. Every language binding is explicitly `available` with a grammar name or `unavailable` with a reason; every unbound grammar asset is explicitly marked orphaned with a reason. This keeps tiny test packs valid while the committed release lock test independently pins `159`, `160`, and the audited upstream commit. `xtask` depends on this shared model; it must not carry a second lock parser.

The audited upstream `MANIFEST.md` ABI summary is stale. Generated `parser.c` is authoritative: ABI 13 has 9 grammars, ABI 14 has 78, and ABI 15 has 72. Upstream also has one detected language without a `ts_factory` (`nim`), three IDs sharing YAML (`yaml`, `kustomize`, `k8s`), and two unbound ObjectScript grammar assets. Therefore 159 active IDs resolve to 157 unique bound grammar assets. These are explicit lock states, never silent count exceptions.

`tools/export_grammar_lock.py` reads pinned upstream:

- `internal/cbm/vendored/grammars/MANIFEST.md`;
- all parser/scanner/header assets;
- `crates/goldeneye-discovery/data/languages.tsv`;
- upstream grammar registry mappings.

It emits one TOML grammar record with name, pinned repository/commit, ABI read from each generated `parser.c`, relative asset paths, framed SHA-256 source hash, scanner language, license files, verdict, and optional explicit orphan reason. It emits 160 language bindings, including explicit unavailable entries. Output contains no timestamps or absolute paths and sorts every record/path/binding. It refuses ABI outside the runtime-compatible range, missing license, count mismatch, implicit unavailable/orphan state, unresolved available binding, symlink/non-regular assets, or source outside grammar root.

Grammar hashing is exactly `SHA-256("goldeneye-grammar-assets-v1\\0" || repeated(u64_be(path_len) || slash_normalized_utf8_path || u64_be(content_len) || raw_content))` over every copied parser/scanner/header/license asset sorted by path bytes. Length framing prevents path/content concatenation ambiguity; non-UTF-8 or non-normalized paths are rejected.

- [ ] **Step 4: Implement explicit offline sync command**

Add `xtask` workspace member and workspace-local Cargo alias `xtask = "run -p xtask --"`. Provide `grammars verify` (hash/license/provenance only) and `grammars sync` (verify then materialize). Command:

```bash
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars
```

Behavior:

1. never accesses network;
2. canonicalizes source and the destination parent (plus destination when it exists);
3. rejects source/destination overlap in either direction;
4. rejects symlink/reparse or non-regular locked assets;
5. verifies every locked source hash/license before copy;
6. copies only locked parser/scanner/header/license assets;
7. returns a no-op when an existing destination has the same verified `pack-state.json`;
8. rejects an existing mismatched/non-pack destination without deleting or modifying it;
9. writes an absent destination through a temporary sibling then atomic rename;
10. writes `pack-state.json` with lock hash;
11. removes temporary output on failure.

- [ ] **Step 5: Add sync safety/reproducibility tests**

Use a tiny two-grammar fixture. Cover the hash framing golden, clean verify/sync, hash mismatch, missing license, traversal path, stale temp cleanup, identical existing-pack no-op, mismatched/non-pack destination rejection without mutation, deterministic repeated output, and no mutation of source.

- [ ] **Step 6: Update legal ledger**

Record Tree-sitter runtime and six core grammar crate licenses/versions. Record full lock provenance and require all grammar license files to travel with materialized/release packs.

- [ ] **Step 7: Run metadata/materialization gate against the real pinned checkout**

Run:

```bash
python tools/export_grammar_lock.py --check \
  --source .upstream/codebase-memory-mcp \
  --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c \
  --output grammars/full-pack.toml
cargo xtask grammars verify \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars-audit
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; exporter is byte-for-byte reproducible; real pinned assets verify/materialize; six core runtime grammars and audited 159-asset/160-binding metadata pass. This remains pre-GFP evidence, not full provider/release parity.

- [ ] **Step 8: Commit**

```bash
git add .cargo/config.toml Cargo.toml Cargo.lock crates/goldeneye-syntax grammars tools/export_grammar_lock.py xtask THIRD_PARTY.md docs/superfastpowers/plans/GS
git commit -m "[GS-5] build: lock full Tree-sitter grammar pack"
```
