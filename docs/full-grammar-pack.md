# Full Grammar Pack

The default `goldeneye-syntax` build uses the maintained core grammar crates.
The native full provider is an explicit, source-built feature backed by the
audited `codebase-memory-mcp` commit
`2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.

No build-time download is permitted. Acquire the upstream checkout and Rust
dependencies first, cross the offline boundary, then reproduce, materialize,
verify, and compile the pack. Run these commands from the Goldeneye workspace
root and use an empty materialization destination.

## PowerShell

Acquire the exact upstream commit and locked Rust dependencies while network
access is allowed:

```powershell
$Commit = "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c"
git clone --filter=blob:none --no-checkout https://github.com/DeusData/codebase-memory-mcp.git .upstream/codebase-memory-mcp
git -C .upstream/codebase-memory-mcp fetch --depth 1 origin $Commit
git -C .upstream/codebase-memory-mcp checkout --detach $Commit
if ((git -C .upstream/codebase-memory-mcp rev-parse HEAD).Trim() -ne $Commit) { throw "unexpected upstream commit" }
cargo fetch --locked
```

Enter the offline boundary and reproduce every checked-in artifact:

```powershell
$env:CARGO_NET_OFFLINE = "true"
$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars"
python tools/export_grammar_lock.py --check --source .upstream/codebase-memory-mcp --expected-commit $Commit --output grammars/full-pack.toml
cargo xtask grammars sync --lock grammars/full-pack.toml --git-repo .upstream/codebase-memory-mcp --git-prefix internal/cbm/vendored/grammars --dest target/goldeneye-grammars
cargo xtask grammars verify --lock grammars/full-pack.toml --source target/goldeneye-grammars
cargo xtask grammars generate-provider --lock grammars/full-pack.toml --output crates/goldeneye-full-grammars/src/generated.rs --check
cargo xtask grammars generate-notices --lock grammars/full-pack.toml --output grammars/full-pack-license-ledger.md --check
```

Compile and audit the native registry and safe provider:

```powershell
cargo clippy -p goldeneye-full-grammars --all-targets --features compiled -- -D warnings
cargo test -p goldeneye-full-grammars --features compiled
cargo clippy -p goldeneye-syntax --all-targets --no-default-features --features full-grammar-pack -- -D warnings
cargo test -p goldeneye-syntax --no-default-features --features full-grammar-pack
cargo test -p goldeneye-syntax --all-features
cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features
cargo test -p goldeneye-syntax --release --no-default-features --features full-grammar-pack --no-run
```

Return to the cache-free default lane by clearing both variables:

```powershell
Remove-Item Env:GOLDENEYE_GRAMMAR_PACK_DIR -ErrorAction SilentlyContinue
Remove-Item Env:CARGO_NET_OFFLINE -ErrorAction SilentlyContinue
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## POSIX shell

Acquire the exact upstream commit and locked Rust dependencies while network
access is allowed:

```sh
commit=2469ecc3a7a2f80debe296e1f17a1efcfdb9450c
git clone --filter=blob:none --no-checkout https://github.com/DeusData/codebase-memory-mcp.git .upstream/codebase-memory-mcp
git -C .upstream/codebase-memory-mcp fetch --depth 1 origin "$commit"
git -C .upstream/codebase-memory-mcp checkout --detach "$commit"
test "$(git -C .upstream/codebase-memory-mcp rev-parse HEAD)" = "$commit"
cargo fetch --locked
```

Enter the offline boundary and reproduce every checked-in artifact:

```sh
export CARGO_NET_OFFLINE=true
export GOLDENEYE_GRAMMAR_PACK_DIR=target/goldeneye-grammars
python tools/export_grammar_lock.py --check --source .upstream/codebase-memory-mcp --expected-commit "$commit" --output grammars/full-pack.toml
cargo xtask grammars sync --lock grammars/full-pack.toml --git-repo .upstream/codebase-memory-mcp --git-prefix internal/cbm/vendored/grammars --dest target/goldeneye-grammars
cargo xtask grammars verify --lock grammars/full-pack.toml --source target/goldeneye-grammars
cargo xtask grammars generate-provider --lock grammars/full-pack.toml --output crates/goldeneye-full-grammars/src/generated.rs --check
cargo xtask grammars generate-notices --lock grammars/full-pack.toml --output grammars/full-pack-license-ledger.md --check
```

Compile and audit the native registry and safe provider:

```sh
cargo clippy -p goldeneye-full-grammars --all-targets --features compiled -- -D warnings
cargo test -p goldeneye-full-grammars --features compiled
cargo clippy -p goldeneye-syntax --all-targets --no-default-features --features full-grammar-pack -- -D warnings
cargo test -p goldeneye-syntax --no-default-features --features full-grammar-pack
cargo test -p goldeneye-syntax --all-features
cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features
cargo test -p goldeneye-syntax --release --no-default-features --features full-grammar-pack --no-run
```

Return to the cache-free default lane with:

```sh
unset GOLDENEYE_GRAMMAR_PACK_DIR CARGO_NET_OFFLINE
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Feature and artifact boundaries

- Core-only is the default: `cargo test -p goldeneye-syntax` enables
  `core-grammars` and requires no full-pack cache.
- Full-only is explicit: `cargo test -p goldeneye-syntax
  --no-default-features --features full-grammar-pack` links the verified native
  registry and excludes the five maintained core grammar crates.
- Mixed is the collision sentinel: `cargo test -p goldeneye-syntax
  --all-features` links both providers because every full native factory and
  scanner export is namespaced under `goldeneye_full_`.

The locked inventory contains 160 declared language IDs, 159 available
language IDs, 157 unique callable factories, and two ObjectScript orphan
sources. The source pack contains 159 grammar groups, one native-support
group, and 914 total assets. The deterministic license ledger has one direct
license row per grammar plus two native-support license rows for the shared
`common` native-support assets.

The full build compiles locked generated parser/scanner sources and the shared
support headers. No upstream application C code is linked, and there is no
bundled Tree-sitter runtime from `codebase-memory-mcp`; Goldeneye uses the
locked Rust `tree-sitter` runtime dependency.

On MSVC, the verified COBOL scanner needs a bounded compatibility path because
Visual Studio 2017 rejects its two variable-length local arrays. The MSVC-only
COBOL derivation accepts one exact scanner hash and structure, changes only the
two proven bounds in an `OUT_DIR` copy, and fails closed on drift. Other
platforms compile the verified scanner directly. Neither path mutates the
materialized source cache.

## Missing or stale cache recovery

An unset `GOLDENEYE_GRAMMAR_PACK_DIR` fails compiled mode with the exact sync
remediation. Set the variable only after `grammars sync` and `grammars verify`
both succeed.

If verification reports stale state, extra files, missing assets, or hash
drift, do not repair files in place. Materialize into a new empty destination,
verify it, then switch the environment variable. For example:

```powershell
cargo xtask grammars sync --lock grammars/full-pack.toml --git-repo .upstream/codebase-memory-mcp --git-prefix internal/cbm/vendored/grammars --dest target/goldeneye-grammars-rebuilt
cargo xtask grammars verify --lock grammars/full-pack.toml --source target/goldeneye-grammars-rebuilt
$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars-rebuilt"
```

The POSIX equivalent changes only the final assignment:

```sh
export GOLDENEYE_GRAMMAR_PACK_DIR=target/goldeneye-grammars-rebuilt
```

If a generated `--check` command reports drift, regenerate without `--check`
only when the lock change is intentional, inspect the resulting diff, and run
the complete verification sequence again.

## Evidence and release claims

The all-ID audit proves factory availability, namespaced linkage, locked ABI
agreement, `Parser::set_language`, empty-input parsing, and basic lifecycle
coverage. The targeted non-empty fixtures cover selected scanner-sensitive
grammars. This evidence does not prove broad behavioral conformance for every
grammar or scanner token path.

GFP produces a source-built provider and reproducible license ledger. Phase 6
is the release packaging boundary: publishable artifacts must additionally
bundle all locked grammar and support license texts, record platform/compiler
evidence, and run the final binary self-audit. Until then, full-pack CI is
runtime-provider evidence, not final delivery completeness.
