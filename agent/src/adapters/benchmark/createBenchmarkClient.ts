import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import { GatewayCodebaseMemoryClient } from "../codebaseMemoryMcp/GatewayCodebaseMemoryClient.js";
import { StdioCodebaseMemoryClient } from "../codebaseMemoryMcp/StdioCodebaseMemoryClient.js";

export interface BenchmarkClientConfig {
  command: string;
  mcpUrl: string | undefined;
  project: string;
}

export function createBenchmarkClient(config: BenchmarkClientConfig): GcalBackendClient {
  return config.mcpUrl
    ? new GatewayCodebaseMemoryClient({ mcpUrl: config.mcpUrl, project: config.project })
    : new StdioCodebaseMemoryClient({ command: config.command, project: config.project });
}
