### Task 1: Create Workspace and Domain Kernel

<TASK-ID>GF-1</TASK-ID>

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `crates/goldeneye-domain/Cargo.toml`
- Create: `crates/goldeneye-domain/src/lib.rs`

- [ ] **Step 1: Create workspace manifests**

```toml
# Cargo.toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.97"
license = "MIT"
repository = "https://github.com/GoldeneyeZ/goldeneye-tool"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = "deny"
pedantic = "deny"
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.97.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

```toml
# crates/goldeneye-domain/Cargo.toml
[package]
name = "goldeneye-domain"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write failing domain tests**

```rust
#[test]
fn project_id_rejects_empty_value() {
    assert_eq!(ProjectId::new(""), Err(DomainError::EmptyProjectId));
}

#[test]
fn project_id_preserves_valid_value() {
    let id = ProjectId::new("sample").expect("valid project ID");
    assert_eq!(id.as_str(), "sample");
}
```

- [ ] **Step 3: Run test and verify failure**

Run: `cargo test -p goldeneye-domain`

Expected: FAIL because `ProjectId` and `DomainError` are undefined.

- [ ] **Step 4: Implement domain kernel**

```rust
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("project ID must not be empty")]
    EmptyProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DomainError::EmptyProjectId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

- [ ] **Step 5: Verify workspace kernel**

Run: `cargo fmt --check && cargo clippy -p goldeneye-domain --all-targets -- -D warnings && cargo test -p goldeneye-domain`

Expected: all commands exit 0; two tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml crates/goldeneye-domain
git commit -m "build: create Goldeneye Rust workspace"
```
