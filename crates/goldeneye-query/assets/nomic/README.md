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
  `b2d1cc1524bc934c157d9b64afa1d45cf0739c5d9db7e8806ddce7ed48232819`

The loader rejects either asset if its checksum, shape, byte length, token
count, UTF-8 encoding, or token uniqueness differs from this audited bundle.
