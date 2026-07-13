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

Tree-sitter runtime and grammar assets are not yet compiled into Goldeneye's
production Rust runtime. When they enter production:

- The Tree-sitter runtime must retain its MIT notice and Max Brunsfeld
  copyright attribution.
- Every grammar must retain its own upstream license, copyright, repository,
  and pinned revision.
- Grammar-specific notices must be expanded here from the audited provenance
  manifest; a summary license cannot replace per-grammar attribution.

The audited upstream checkout's grammar set includes MIT, CC0-1.0,
Apache-2.0, ISC, and in-house/fork-specific notices. No grammar asset is
covered merely by Goldeneye's project license.

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
| `itoa` | 1.0.18 | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `proc-macro2` | 1.0.106 | MIT OR Apache-2.0 |
| `quote` | 1.0.46 | MIT OR Apache-2.0 |
| `serde` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.150 | MIT OR Apache-2.0 |
| `syn` | 2.0.118 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.18 | MIT OR Apache-2.0 |
| `thiserror-impl` | 2.0.18 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
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
