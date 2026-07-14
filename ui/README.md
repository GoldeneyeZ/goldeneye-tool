# Goldeneye graph UI

This is the standalone browser UI ported from
`DeusData/codebase-memory-mcp/graph-ui` at commit
`2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`. It preserves project selection,
graph browsing, search, filters, dead-code and missed-coverage views, display
density controls, server-computed 3D layout coordinates, project management,
ADR editing, and process/log controls.

## Development

```sh
npm ci
npm test
npm run build
```

The default API is same-origin. To serve it below a prefix, set
`globalThis.__GOLDENEYE_UI_CONFIG__.apiBasePath` in `runtime-config.js`, set
the `goldeneye-api-base` meta tag, or provide `VITE_API_BASE_PATH` at build
time. For a UI asset prefix, set `GOLDENEYE_UI_BASE_PATH` while building.
Only `src/api/basePath.ts` joins API routes to this prefix.

Every source asset is recorded in `asset-manifest.json` and
`checksums.sha256`. A build emits the same pair inside `dist/` for the Rust
HTTP embedder. Original upstream source hashes are in
`UPSTREAM_SOURCES.sha256`.

License materials are copied into `dist/legal/`. `THIRD_PARTY_LICENSES.md`
contains the verbatim license files of the production npm dependency tree.
