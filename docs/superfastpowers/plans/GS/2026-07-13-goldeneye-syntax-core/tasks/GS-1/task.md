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
