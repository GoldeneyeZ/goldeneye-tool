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
