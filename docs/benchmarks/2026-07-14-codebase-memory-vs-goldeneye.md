# codebase-memory-mcp vs Goldeneye baseline

Date: 2026-07-14

## Scope

End-to-end Windows x64 comparison: release binary size, cold full indexing,
process-tree peak RSS, cache size, warm query latency, and a small result-quality
smoke suite.

This is a development baseline, not a publication-grade relevance evaluation.

## Compared builds

| Engine | Version | SHA-256 | Binary size |
|---|---:|---|---:|
| codebase-memory-mcp | 0.9.0 | `9a205fa5ae759fbc866bfe1554f0c05a303be9ae6e0a00f94d875dc0c25e0680` | 273,333,760 B |
| Goldeneye | 0.1.0, repository HEAD `14cdb59` | `26c7887e4bac48cbd738aa07420248fd63f25f10dc69069bb874eb62c90e3499` | 15,499,264 B |

Goldeneye is 17.6x smaller (94.3% fewer bytes).

The exact vendored upstream commit, `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`,
could not be built locally because native GNU/Clang tooling is absent and WSL2
cannot start (`HCS_E_HYPERV_NOT_INSTALLED`). The comparator is the nearest
official release, v0.9.0 at `b637e33`, 141 commits behind the vendored commit.
The globally installed v0.8.1 binary was rejected as stale.

## Corpus and method

- Corpus: `D:/Dev/IdeaProjects/terax-ai`
- Commit: `2765b081df4e57e9b635c5b94fbccd110e7baac2`
- Tracked files: 640
- Tracked bytes: 11,600,329
- Working tree: dirty; both engines saw the same working directory during the run
- Index mode: `full`, `persistence: false`
- Cold runs: 3 per engine, alternated `C,G,G,C,C,G`
- Cache: unique fresh `CBM_CACHE_DIR` per run
- RSS: summed working set for server and all descendants, sampled every 20 ms
- Query process: persistent MCP session; 3 warmups, then 20 measured repetitions
- Startup update thread: 6-second wait after `initialize` before indexing

Reusable runner: `node tools/benchmark-competitors.mjs --repo <corpus-path>`.
It writes machine-readable latency, response-size, cardinality, version, index,
and cache metrics under `target/benchmarks/`.

## Cold full-index results

| Metric | codebase-memory-mcp | Goldeneye | Result |
|---|---:|---:|---|
| Wall-time runs | 3.429, 2.200, 2.540 s | 13.943, 13.506, 15.602 s | — |
| Median wall time | **2.540 s** | 13.943 s | codebase-memory-mcp 5.49x faster |
| Peak process-tree RSS runs | 235.99, 234.64, 230.86 MiB | 123.12, 132.59, 125.34 MiB | — |
| Median peak RSS | 234.64 MiB | **125.34 MiB** | Goldeneye 46.6% lower |
| Cache bytes | **21,082,112** | 87,420,280 | Goldeneye cache 4.15x larger |

Reported graph sizes differ substantially and must not be treated as quality:

- codebase-memory-mcp: 6,750 nodes, 21,512 edges
- Goldeneye: 25,654 nodes, 31,581 edges, 485 files

The engines use different graph ontologies and counting policies.

## Corrected warm-query latency

Milliseconds; lower is better.

| Operation | codebase-memory-mcp p50 / p95 | Goldeneye p50 / p95 | p50 comparison |
|---|---:|---:|---:|
| Exact `search_graph` (`fs_search`) | 2.400 / 3.338 | 442.753 / 507.048 | Goldeneye 184.5x slower |
| BM25 `search_graph` (`fs search`) | 7.230 / 7.887 | 123.471 / 148.129 | Goldeneye 17.1x slower |
| `search_code` (`node_modules`) | 630.148 / 830.206 | **539.719 / 573.838** | Goldeneye 14.4% faster |
| `trace_path` (`fs_search`) | 1.033 / 1.102 | 472.939 / 510.555 | Goldeneye 457.8x slower |
| Exact-QN `get_code_snippet` | 0.524 / 0.659 | 453.071 / 541.761 | Goldeneye 864.6x slower |
| `get_architecture` | 11.743 / 12.542 | 522.337 / 636.261 | Goldeneye 44.5x slower |
| Common Cypher `MATCH (n) RETURN n LIMIT 20` | 59.667 / 72.793 | 2,753.843 / 2,973.681 | Goldeneye 46.2x slower |

## Post-fix Goldeneye warm-query latency

After adding a process-shared, generation-aware graph snapshot, cached degree and
adjacency indexes, exact anchored-name lookup, and bounded simple-Cypher row
retention, the same persistent-session protocol (3 warmups, 20 measured calls)
produced:

| Operation | Baseline Goldeneye p50 / p95 | Post-fix p50 / p95 | p50 speedup |
|---|---:|---:|---:|
| Exact `search_graph` (`fs_search`) | 442.753 / 507.048 | **2.404 / 2.808** | **184.2x** |
| BM25 `search_graph` (`fs search`) | 123.471 / 148.129 | **10.945 / 12.186** | **11.3x** |
| `trace_path` (`fs_search`) | 472.939 / 510.555 | **15.768 / 17.801** | **30.0x** |
| Exact-QN `get_code_snippet` | 453.071 / 541.761 | **1.848 / 2.101** | **245.2x** |
| `get_architecture` | 522.337 / 636.261 | **40.873 / 53.867** | **12.8x** |
| Common Cypher `MATCH (n) RETURN n LIMIT 20` | 2,753.843 / 2,973.681 | **147.264 / 153.218** | **18.7x** |

The fixed exact-symbol p50 is effectively tied with codebase-memory-mcp
(2.404 ms versus 2.400 ms). The remaining largest gaps are architecture and
general Cypher evaluation; both now avoid repeated graph deserialization, but
still evaluate their result across the in-memory graph.

An initial query pass was discarded: it sent upstream-only `format: "json"` to
Goldeneye and used `query` instead of `pattern` for `search_code`. The table above
contains corrected reruns only.

## Quality smoke results

Both engines:

- returned expected `fs_search` function for exact search;
- placed expected filesystem-search implementation in natural-search top 10;
- returned non-empty `search_code` and `trace_path` results;
- returned the expected source for engine-specific exact qualified names.

Top-10 normalized result overlap:

| Query | Jaccard | Common / union |
|---|---:|---:|
| Exact symbol | 0.500 | 1 / 2 |
| Natural search | 0.053 | 1 / 19 |
| Code search | 0.364 | 4 / 11 |

Low overlap shows materially different ranking/ontology behavior; it does not by
itself identify the better result set. A curated multi-query oracle is needed for
MRR, nDCG@10, recall@10, and edge precision/recall.

## Limitations

- Corpus working tree was dirty, although both engines read the same directory.
- Cold indexing used three runs; query latency used one indexed session per engine.
- Quality coverage used only three search cases and one known target symbol.
- Comparator v0.9.0 is 141 commits behind Goldeneye's pinned upstream source.
- Windows background load, CPU affinity, and power policy were not controlled.

## Compatibility observations

- `get_code_snippet("fs_search")`: codebase-memory-mcp selected a candidate;
  Goldeneye correctly reported ambiguity. Exact engine-specific qualified names
  worked on both and were used for fair latency.
- Aggregate query with `ORDER BY` worked on codebase-memory-mcp but Goldeneye
  returned `Cypher syntax error at byte 26: unsupported trailing clause`.
- Goldeneye uses `src_tauri` in qualified names where codebase-memory-mcp uses
  `src-tauri`; callers must not assume cross-engine qualified-name identity.

## Baseline verdict

Goldeneye already wins binary size, indexing peak memory, and this corpus's
`search_code` latency. codebase-memory-mcp currently wins full-index time, cache
size, and every graph-backed warm-query latency by large margins. Goldeneye's
highest-value optimization target is repeated full-graph loading/deserialization
in graph-backed query paths, followed by cache-density and BM25 indexing/ranking
parity.

## Next rigorous run

1. Freeze clean detached corpus snapshots.
2. Build exact upstream `2469ecc` and current Goldeneye with native release flags.
3. Use at least one TS/Rust corpus and one Go/TS scale corpus.
4. Run 5 cold indexes and randomized 30+ query repetitions.
5. Add a curated oracle for search relevance, trace edges, and snippet spans.
