# Third-Party Notices

This ledger records third-party material used by Goldeneye. Versions below are
locked by `Cargo.lock`; source distributions must retain the corresponding
license texts.

## codebase-memory-mcp

- Project: [DeusData/codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp)
- Audited commit: `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- License: MIT
- Copyright: (c) 2025 DeusData
- Retained license: `NOTICE`

## Tree-sitter Runtime and Grammars

Goldeneye's core syntax runtime currently embeds five MIT-licensed grammar
packages exposing six runtime grammars:

| Runtime grammar | Rust package | Version | License |
|---|---|---:|---|
| Go | `tree-sitter-go` | 0.25.0 | MIT |
| JavaScript | `tree-sitter-javascript` | 0.25.0 | MIT |
| Python | `tree-sitter-python` | 0.25.0 | MIT |
| Rust | `tree-sitter-rust` | 0.24.2 | MIT |
| TypeScript | `tree-sitter-typescript` | 0.23.2 | MIT |
| TSX | `tree-sitter-typescript` | 0.23.2 | MIT |

The parser runtime is `tree-sitter` 0.26.11 (MIT, Max Brunsfeld); grammar
packages share the Tree-sitter MIT notice or carry their repository's retained
MIT notice. `tree-sitter-language` 0.1.7 (MIT) is part of that locked runtime
closure.

### Full grammar-pack provenance

`grammars/full-pack.toml` is deterministic metadata derived from
`DeusData/codebase-memory-mcp` commit
`2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`. It locks 159 grammar directories,
907 compilation/license files, and 160 explicit language-binding records. The
lock preserves the heterogeneous `MANIFEST.md` provenance verdicts, missing
upstream-revision reasons, and local source-patch notes; source hashes cover the
patched vendored bytes exactly as stored in that commit's Git blobs, without
checkout line-ending normalization. Export, verification, and materialization
disable replacement objects and lazy object fetching, accept only regular Git
blob modes `100644`/`100755`, and stream the lock's exact commit through Git
object plumbing. The real-pack commands therefore use `--git-repo
.upstream/codebase-memory-mcp --git-prefix
internal/cbm/vendored/grammars`; directory `--source` mode is retained only for
tiny fixtures or deliberately prepared byte-stable directories.

Every grammar directory has a directly locked `LICENSE`. Offline materialized
packs and any future release pack must carry every locked per-grammar license
file beside its compilation assets. The audited set includes MIT, CC0-1.0,
Apache-2.0, ISC, and in-house/fork-specific notices. No grammar asset is covered
merely by Goldeneye's project license, and a summary notice cannot replace
those per-grammar texts. This lock/materialization support is metadata only;
it does not claim that the full 159-grammar provider is linked into the runtime.

## Generated Language Registry Data

`crates/goldeneye-discovery/data/languages.tsv` is MIT-derived data generated
by `tools/export_upstream_languages.py` from these files in the audited
`codebase-memory-mcp` commit above:

- `internal/cbm/cbm.h` (language enum order);
- `src/discover/language.c` (display names, extension mappings, exact
  filenames, and compound extensions).

The generated TSV header records the upstream repository, full audited commit,
and MIT provenance. Goldeneye retains the upstream MIT license in `NOTICE`.
Regenerate the data without changing provenance using:

```text
python tools/export_upstream_languages.py --upstream .upstream/codebase-memory-mcp --output crates/goldeneye-discovery/data/languages.tsv
```

## Rust Crates

| Crate | Version | License |
|---|---:|---|
| `aho-corasick` | 1.1.4 | Unlicense OR MIT |
| `ambient-authority` | 0.0.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `arrayref` | 0.3.9 | BSD-2-Clause |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 |
| `bitflags` | 2.13.0 | MIT OR Apache-2.0 |
| `blake3` | 1.8.5 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| `block-buffer` | 0.10.4 | MIT OR Apache-2.0 |
| `bstr` | 1.12.3 | MIT OR Apache-2.0 |
| `cap-primitives` | 4.0.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `cc` | 1.2.67 | MIT OR Apache-2.0 |
| `cfg-if` | 1.0.4 | MIT OR Apache-2.0 |
| `constant_time_eq` | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| `cpufeatures` | 0.2.17 | MIT OR Apache-2.0 |
| `cpufeatures` | 0.3.0 | MIT OR Apache-2.0 |
| `crossbeam-deque` | 0.8.7 | MIT OR Apache-2.0 |
| `crossbeam-epoch` | 0.9.20 | MIT OR Apache-2.0 |
| `crossbeam-utils` | 0.8.22 | MIT OR Apache-2.0 |
| `crypto-common` | 0.1.7 | MIT OR Apache-2.0 |
| `digest` | 0.10.7 | MIT OR Apache-2.0 |
| `equivalent` | 1.0.2 | Apache-2.0 OR MIT |
| `errno` | 0.3.14 | MIT OR Apache-2.0 |
| `fastrand` | 2.4.1 | Apache-2.0 OR MIT |
| `find-msvc-tools` | 0.1.9 | MIT OR Apache-2.0 |
| `fs-set-times` | 0.20.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `generic-array` | 0.14.7 | MIT |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 |
| `globset` | 0.4.18 | Unlicense OR MIT |
| `hashbrown` | 0.17.1 | MIT OR Apache-2.0 |
| `ignore` | 0.4.28 | Unlicense OR MIT |
| `indexmap` | 2.14.0 | Apache-2.0 OR MIT |
| `io-extras` | 0.19.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `io-lifetimes` | 2.0.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `io-lifetimes` | 3.0.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `ipnet` | 2.12.0 | MIT OR Apache-2.0 |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 |
| `libc` | 0.2.186 | MIT OR Apache-2.0 |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `log` | 0.4.33 | MIT OR Apache-2.0 |
| `maybe-owned` | 0.3.4 | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 |
| `proc-macro2` | 1.0.106 | MIT OR Apache-2.0 |
| `quote` | 1.0.46 | MIT OR Apache-2.0 |
| `r-efi` | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `regex` | 1.13.0 | MIT OR Apache-2.0 |
| `regex-automata` | 0.4.15 | MIT OR Apache-2.0 |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustix-linux-procfs` | 0.1.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `same-file` | 1.0.6 | Unlicense/MIT |
| `serde` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.150 | MIT OR Apache-2.0 |
| `serde_spanned` | 1.1.1 | MIT OR Apache-2.0 |
| `sha2` | 0.10.9 | MIT OR Apache-2.0 |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 |
| `streaming-iterator` | 0.1.9 | MIT OR Apache-2.0 |
| `syn` | 2.0.118 | MIT OR Apache-2.0 |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.18 | MIT OR Apache-2.0 |
| `thiserror-impl` | 2.0.18 | MIT OR Apache-2.0 |
| `toml` | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 |
| `toml_datetime` | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 |
| `toml_parser` | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| `toml_writer` | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| `tree-sitter` | 0.26.11 | MIT |
| `tree-sitter-go` | 0.25.0 | MIT |
| `tree-sitter-javascript` | 0.25.0 | MIT |
| `tree-sitter-language` | 0.1.7 | MIT |
| `tree-sitter-python` | 0.25.0 | MIT |
| `tree-sitter-rust` | 0.24.2 | MIT |
| `tree-sitter-typescript` | 0.23.2 | MIT |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `version_check` | 0.9.5 | MIT/Apache-2.0 |
| `walkdir` | 2.5.0 | Unlicense/MIT |
| `winapi-util` | 0.1.11 | Unlicense OR MIT |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.60.2 | MIT OR Apache-2.0 |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 |
| `windows-targets` | 0.53.5 | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnu` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_i686_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | 0.53.1 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.53.1 | MIT OR Apache-2.0 |
| `winnow` | 0.7.15 | MIT |
| `winnow` | 1.0.3 | MIT |
| `winx` | 0.36.4 | Apache-2.0 WITH LLVM-exception |
| `zmij` | 1.0.22 | MIT |

Regenerate this section whenever `Cargo.lock` changes. Transitive crates are
third-party dependencies even when Goldeneye does not import them directly.

### Repository Discovery Dependency Closure

Versions and licenses below come from locked package metadata. Source links
identify the corresponding upstream project for `ignore 0.4.28` and every
normal dependency in its resolved closure.

| Crate | Version | License | Source |
|---|---:|---|---|
| `ignore` | 0.4.28 | Unlicense OR MIT | [BurntSushi/ripgrep — ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) |
| `aho-corasick` | 1.1.4 | Unlicense OR MIT | [BurntSushi/aho-corasick](https://github.com/BurntSushi/aho-corasick) |
| `bstr` | 1.12.3 | MIT OR Apache-2.0 | [BurntSushi/bstr](https://github.com/BurntSushi/bstr) |
| `crossbeam-deque` | 0.8.7 | MIT OR Apache-2.0 | [crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-epoch` | 0.9.20 | MIT OR Apache-2.0 | [crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-utils` | 0.8.22 | MIT OR Apache-2.0 | [crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `globset` | 0.4.18 | Unlicense OR MIT | [BurntSushi/ripgrep — globset](https://github.com/BurntSushi/ripgrep/tree/master/crates/globset) |
| `log` | 0.4.33 | MIT OR Apache-2.0 | [rust-lang/log](https://github.com/rust-lang/log) |
| `memchr` | 2.8.3 | Unlicense OR MIT | [BurntSushi/memchr](https://github.com/BurntSushi/memchr) |
| `regex-automata` | 0.4.15 | MIT OR Apache-2.0 | [rust-lang/regex](https://github.com/rust-lang/regex) |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 | [rust-lang/regex](https://github.com/rust-lang/regex) |
| `same-file` | 1.0.6 | Unlicense/MIT | [BurntSushi/same-file](https://github.com/BurntSushi/same-file) |
| `walkdir` | 2.5.0 | Unlicense/MIT | [BurntSushi/walkdir](https://github.com/BurntSushi/walkdir) |
| `winapi-util` | 0.1.11 | Unlicense OR MIT | [BurntSushi/winapi-util](https://github.com/BurntSushi/winapi-util) |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
