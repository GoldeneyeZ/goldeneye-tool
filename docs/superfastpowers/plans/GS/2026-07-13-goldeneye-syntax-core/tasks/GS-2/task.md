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
