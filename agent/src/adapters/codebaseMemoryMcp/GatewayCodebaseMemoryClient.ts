import type { SearchOptions, TraceEdge, TraceOptions } from "../../domain/types.js";
import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import {
  CodebaseMemoryMcpClient,
  type McpToolInvoker,
} from "./CodebaseMemoryMcpClient.js";
import { gatewayInvoke } from "./gatewayJsonRpc.js";

export interface GatewayClientConfig {
  mcpUrl: string;
  project: string;
  fetch?: typeof globalThis.fetch;
}

export class GatewayCodebaseMemoryClient implements GcalBackendClient {
  private readonly client: CodebaseMemoryMcpClient;

  constructor(config: GatewayClientConfig) {
    const fetchImpl = config.fetch ?? globalThis.fetch;
    const invoker: McpToolInvoker = {
      invoke: (toolName, args) =>
        gatewayInvoke({
          mcpUrl: config.mcpUrl,
          toolId: `codebase-memory-mcp::${toolName}`,
          args,
          fetch: fetchImpl,
        }),
    };

    this.client = new CodebaseMemoryMcpClient(config.project, invoker);
  }

  search(query: string, options: Partial<SearchOptions>) {
    return this.client.search(query, options);
  }

  symbol(nameRegex: string, options: Partial<SearchOptions>) {
    return this.client.symbol(nameRegex, options);
  }

  get(qualifiedName: string) {
    return this.client.get(qualifiedName);
  }

  callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.client.callers(qualifiedName, options);
  }

  callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.client.callees(qualifiedName, options);
  }

  arch(): Promise<unknown> {
    return this.client.arch();
  }

  status(): Promise<unknown> {
    return this.client.status();
  }

  index(repoPath: string): Promise<unknown> {
    return this.client.index(repoPath);
  }

  projects() {
    return this.client.projects();
  }
}
