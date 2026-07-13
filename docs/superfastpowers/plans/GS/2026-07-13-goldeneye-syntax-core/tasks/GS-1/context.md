# Context for GS-1

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-1`
**Plan Commit SHA:** `4305d0c`

## Starting Context

- `crates/goldeneye-syntax/Cargo.toml`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/grammar.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/core_grammars.rs`: starting point named by implementation plan.
- `crates/goldeneye-domain/tests/language_id.rs`: starting point named by implementation plan.
- `crates/goldeneye-discovery/tests/domain_ids.rs`: starting point named by implementation plan.
- `crates/goldeneye-domain/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-discovery/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-discovery/Cargo.toml`: starting point named by implementation plan.
- `Cargo.toml`: starting point named by implementation plan.
- `Cargo.lock`: starting point named by implementation plan.
- `toml
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
`: starting point named by implementation plan.
- `

- [ ] **Step 2: Write failing shared-language-ID tests**

Move the existing `: starting point named by implementation plan.
- ` contract into `: starting point named by implementation plan.
- `. Test non-empty preservation, empty rejection, ordering/hash behavior, and type identity through `: starting point named by implementation plan.
- `. Discovery must use `: starting point named by implementation plan.
- `; it must not define a wrapper or conversion copy.

- [ ] **Step 3: Run shared-ID tests and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because domain does not own `: starting point named by implementation plan.
- ` and discovery does not re-export it.

- [ ] **Step 4: Migrate the ID and make the constructor error change explicit**

Add `: starting point named by implementation plan.
- ` and its typed validation error to domain, add the domain dependency to discovery, delete discovery's duplicate definition, and publicly re-export the domain type from discovery. This intentionally changes `: starting point named by implementation plan.
- ` from `: starting point named by implementation plan.
- ` to the domain validation error; update the existing discovery unit assertion and document the 0.1 pre-release API change. Callers using valid IDs retain exact type identity. Run the entire discovery suite to prove registry/walker behavior remains unchanged.

- [ ] **Step 5: Write failing provider tests**

`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

Fixtures contain one small valid declaration for each of Go, JavaScript, Python, Rust, TypeScript, and TSX. Metadata fixtures pin the exact crate package/version declared in the manifest.

- [ ] **Step 6: Run provider tests and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because provider/crate does not exist.

- [ ] **Step 7: Implement provider boundary**

`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

`: starting point named by implementation plan.
- `/`: starting point named by implementation plan.
- ` derive the owned debug/clone/equality traits needed by snapshots and metadata tests; no metadata field borrows provider-local storage.

`: starting point named by implementation plan.
- ` maps exact IDs to grammar constants:

- `: starting point named by implementation plan.
- `
- `: starting point named by implementation plan.
- `
- `: starting point named by implementation plan.
- `
- `: starting point named by implementation plan.
- `
- `: starting point named by implementation plan.
- `
- `: starting point named by implementation plan.
- `

Convert each `: starting point named by implementation plan.
- ` through `: starting point named by implementation plan.
- ` and record the generated grammar ABI from `: starting point named by implementation plan.
- ` through checked `: starting point named by implementation plan.
- ` conversion. Do not make the provider contract depend on grammar-crate-only `: starting point named by implementation plan.
- ` JSON: the pinned full-pack assets expose node/field metadata through the compiled `: starting point named by implementation plan.
- ` API but do not carry those JSON files. Return typed `: starting point named by implementation plan.
- ` for other IDs. Sort `: starting point named by implementation plan.
- ` lexically.

- [ ] **Step 8: Verify provider**

Run: `: starting point named by implementation plan.
- `

Expected: shared ID migration, all discovery regression tests, provider tests, and six grammar parse tests pass.

- [ ] **Step 9: Commit**

`: starting point named by implementation plan.
- `bash
git add Cargo.toml Cargo.lock crates/goldeneye-domain crates/goldeneye-discovery crates/goldeneye-syntax
git commit -m "[GS-1] feat: add Tree-sitter grammar provider"
`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Pending implementation, review evidence, final commit, and controller verification.
