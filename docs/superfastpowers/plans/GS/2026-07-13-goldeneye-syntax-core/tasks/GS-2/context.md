# Context for GS-2

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-2`
**Plan Commit SHA:** `4305d0c`

## Starting Context

- `crates/goldeneye-domain/src/syntax.rs`: starting point named by implementation plan.
- `crates/goldeneye-domain/tests/syntax_types.rs`: starting point named by implementation plan.
- `crates/goldeneye-domain/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-domain/Cargo.toml`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/engine.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/Cargo.toml`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/diagnostics.rs`: starting point named by implementation plan.
- `ContentHash::of`: starting point named by implementation plan.
- `Generation`: starting point named by implementation plan.
- `ByteSpan`: starting point named by implementation plan.
- `SourcePoint { row, column_bytes }`: starting point named by implementation plan.
- `row`: starting point named by implementation plan.
- `ProjectRelativePath`: starting point named by implementation plan.
- `.`: starting point named by implementation plan.
- `..`: starting point named by implementation plan.
- `GrammarFingerprint`: starting point named by implementation plan.
- `LocatorScope`: starting point named by implementation plan.
- `AncestorStep`: starting point named by implementation plan.
- `NodeAnchor`: starting point named by implementation plan.
- `NodeLocator`: starting point named by implementation plan.
- `u64`: starting point named by implementation plan.
- `usize`: starting point named by implementation plan.
- `cargo test -p goldeneye-domain --test syntax_types`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

These types derive/implement owned equality, hashing where meaningful, and compact validated Serde without importing Tree-sitter or filesystem APIs. Deserialization must invoke constructors/invariant checks, never bypass them through transparent unchecked derives. Add compatible validated Serde to existing `: starting point named by implementation plan.
- ` and `: starting point named by implementation plan.
- `, add `: starting point named by implementation plan.
- ` and `: starting point named by implementation plan.
- ` plus test-only `: starting point named by implementation plan.
- ` to `: starting point named by implementation plan.
- `, and re-export the syntax types from its root.

- [ ] **Step 4: Write failing snapshot/raw-byte/diagnostic tests**

`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

- [ ] **Step 5: Run engine tests and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because engine/snapshot types are undefined.

- [ ] **Step 6: Implement parser reuse and immutable snapshots**

Use one thread-local `: starting point named by implementation plan.
- `. For every parse:

1. obtain grammar from provider;
2. get/create parser by language ID;
3. set exact language;
4. parse raw `: starting point named by implementation plan.
- `;
5. return typed `: starting point named by implementation plan.
- ` if Tree-sitter returns `: starting point named by implementation plan.
- `;
6. traverse tree once for diagnostics;
7. store immutable source/tree/hash/generation/language/grammar-fingerprint metadata.

`: starting point named by implementation plan.
- ` owns `: starting point named by implementation plan.
- ` and `: starting point named by implementation plan.
- `. It exposes borrowed root/source and scalar metadata; it never exposes mutable tree access. Project/path are intentionally external. Syntax later combines a current `: starting point named by implementation plan.
- ` with snapshot guards into `: starting point named by implementation plan.
- `, so a resolver can verify every scope field.

Grammar fingerprints are canonical and tested: core uses provider `: starting point named by implementation plan.
- `, grammar language ID, revision `: starting point named by implementation plan.
- `, and ABI; full-pack sources later use provider `: starting point named by implementation plan.
- `, locked grammar asset name, revision source hash, and ABI.

- [ ] **Step 7: Implement diagnostic traversal**

`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

Traverse iteratively with `: starting point named by implementation plan.
- `. Count every error/missing node, retain exactly the first 128 in deterministic source/preorder, and expose total/retained/truncated independently. Add focused assertions for an error node, a missing node, zero-width missing span, multibyte byte columns, exact cap, total count, and retained ordering.

- [ ] **Step 8: Add validated incremental reparse tests and implementation**

Test insertion inside the multibyte Rust identifier in `: starting point named by implementation plan.
- `. The edit point must use byte column 8, not character column 7. Assert:

- new source/hash/generation;
- unchanged root kind;
- no errors;
- changed range reported by `: starting point named by implementation plan.
- ` is bounded around edit.

`: starting point named by implementation plan.
- ` validates old/new byte bounds, byte deltas, and zero-based byte points against old/new raw source; clones the previous tree; applies `: starting point named by implementation plan.
- `; and passes the edited tree to the parser with new bytes. It returns `: starting point named by implementation plan.
- `, computed from the edited old tree against the new tree without mutating the prior snapshot. It rejects inconsistent edits, language changes, and non-increasing generations with typed errors.

- [ ] **Step 9: Verify engine**

Run: `: starting point named by implementation plan.
- `

Expected: domain identity/Serde, valid, malformed, invalid-UTF-8 raw-byte, diagnostic-cap/order, and multibyte incremental tests pass.

- [ ] **Step 10: Commit**

`: starting point named by implementation plan.
- `bash
git add Cargo.lock crates/goldeneye-domain crates/goldeneye-syntax
git commit -m "[GS-2] feat: parse immutable syntax snapshots"
`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Pending implementation, review evidence, final commit, and controller verification.
