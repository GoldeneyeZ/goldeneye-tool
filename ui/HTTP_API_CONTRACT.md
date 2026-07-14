# HTTP and RPC contract

The UI expects JSON responses from the following routes. A configured API base
path is prepended without changing any route or query parameter.

| Method | Route | Purpose |
| --- | --- | --- |
| POST | `/rpc` | MCP-compatible JSON-RPC `tools/call` |
| GET | `/api/layout` | Server-computed graph coordinates; `project`, `max_nodes`, optional `graph=missed` |
| GET | `/api/repo-info` | Repository branch and safe HTTPS deep-link bases |
| POST | `/api/index` | Start indexing `root_path`, optional `project_name` |
| GET | `/api/index-status` | Background indexing jobs |
| GET | `/api/ui-config` | Language and optional upstream issue URL |
| DELETE | `/api/project` | Delete the named project |
| GET | `/api/browse` | Browse server-local directories |
| GET/POST | `/api/adr` | Read or save project ADR text |
| GET | `/api/project-health` | Project database health |
| GET | `/api/processes` | Goldeneye process list |
| GET | `/api/logs` | Recent process logs |
| POST | `/api/process-kill` | Terminate the selected process |

`/rpc` must support `list_projects`, `get_graph_schema`, and
`get_code_snippet`. MCP responses may be returned directly or wrapped as
`result.content[0].text`.

The Rust host still needs to serve `index.html` without caching, serve all
other manifest assets with their content types, and expose the routes above.
Coordinates must remain stable for a stable graph; the UI renders backend
`x/y/z` values without recalculating the force layout.
