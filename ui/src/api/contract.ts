export const HTTP_API_CONTRACT = [
  { method: "POST", path: "/rpc" },
  { method: "GET", path: "/api/layout" },
  { method: "GET", path: "/api/repo-info" },
  { method: "POST", path: "/api/index" },
  { method: "GET", path: "/api/index-status" },
  { method: "GET", path: "/api/ui-config" },
  { method: "DELETE", path: "/api/project" },
  { method: "GET", path: "/api/browse" },
  { method: "GET", path: "/api/adr" },
  { method: "POST", path: "/api/adr" },
  { method: "GET", path: "/api/project-health" },
  { method: "GET", path: "/api/processes" },
  { method: "GET", path: "/api/logs" },
  { method: "POST", path: "/api/process-kill" },
] as const;

export const MCP_TOOL_CONTRACT = [
  "list_projects",
  "get_graph_schema",
  "get_code_snippet",
] as const;
