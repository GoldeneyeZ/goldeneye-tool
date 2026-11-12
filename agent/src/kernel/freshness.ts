import type { GraphNode, ToolClient } from "./codeGraph.js";

/**
 * Tells whether a file changed after it was indexed. The backend refuses to
 * serve a snippet from such a file, so one snippet request for any symbol in
 * the file answers the question; the answer is cached per file.
 */
export class FreshnessProbe {
  private readonly cache = new Map<string, Promise<boolean>>();

  constructor(private readonly client: ToolClient) {}

  isStale(probe: GraphNode): Promise<boolean> {
    let pending = this.cache.get(probe.filePath);
    if (pending === undefined) {
      pending = this.ask(probe);
      this.cache.set(probe.filePath, pending);
    }
    return pending;
  }

  private async ask(probe: GraphNode): Promise<boolean> {
    try {
      await this.client.tool("get_code_snippet", { qualified_name: probe.qualifiedName });
      return false;
    } catch (error) {
      // Any other failure (oversized snippet, unknown symbol) says nothing
      // about freshness; assume the indexed lines still hold.
      return /\bstale\b/i.test(error instanceof Error ? error.message : String(error));
    }
  }
}
