import {
  StdioCodebaseMemoryClient,
  type StdioClientConfig,
} from "../codebaseMemoryMcp/StdioCodebaseMemoryClient.js";
import type { SnippetChunkOptions } from "../../domain/types.js";

export class StdioGoldeneyeClient extends StdioCodebaseMemoryClient {
  constructor(config: StdioClientConfig) {
    const env = {
      ...process.env,
      ...config.env,
      GOLDENEYE_WATCHER_ENABLED: "0",
    };
    super({ ...config, env });
  }

  getSnippetManifest(qualifiedName: string, chunkBytes: number) {
    return this.client.getSnippetManifest(qualifiedName, chunkBytes);
  }

  getSnippetChunk(qualifiedName: string, options: SnippetChunkOptions) {
    return this.client.getSnippetChunk(qualifiedName, options);
  }
}
