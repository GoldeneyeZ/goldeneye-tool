# Nomic code-token vectors

Runtime assets for `nomic-ai/nomic-embed-code`, carried forward from the audited
`DeusData/codebase-memory-mcp` implementation. They are loaded from disk and are
not embedded into Goldeneye binaries.

- Source: <https://huggingface.co/nomic-ai/nomic-embed-code>
- License: Apache-2.0 (see `LICENSE` and `NOTICE`)
- Shape: 40,856 tokens x 768 signed int8 coordinates
- Encoding: 8-byte little-endian `[count, dimension]` header, then row-major
  int8 values scaled by 127
- `code_vectors.bin` SHA-256:
  `c76bba4c5032323ded6202053af5afdbbac12f6d920c691b3b3b4cd708f99e83`
- `code_tokens.txt` SHA-256:
  `c928f5e2f9dd85f2294a50a05dd9f2f8bc95192727579aa16b062ff8ef301d25`

The loader rejects either asset if its checksum, shape, byte length, token
count, UTF-8 encoding, or non-empty token uniqueness differs from this audited
bundle. Eleven empty vocabulary rows are preserved at their original vector
indices and excluded from lookup, matching the upstream map construction.
