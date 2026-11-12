import { readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

/** Reads working-tree files under one project root, caching per invocation. */
export class SourceReader {
  private readonly cache = new Map<string, Promise<string[] | undefined>>();

  constructor(private readonly root: string) {}

  lines(filePath: string): Promise<string[] | undefined> {
    let pending = this.cache.get(filePath);
    if (pending === undefined) {
      pending = this.load(filePath);
      this.cache.set(filePath, pending);
    }
    return pending;
  }

  /** 1-based inclusive line range; clamps to the file. */
  async slice(filePath: string, startLine: number, endLine: number): Promise<string[] | undefined> {
    const lines = await this.lines(filePath);
    if (lines === undefined) return undefined;
    return lines.slice(Math.max(0, startLine - 1), Math.max(0, endLine));
  }

  private async load(filePath: string): Promise<string[] | undefined> {
    const absolute = resolve(this.root, filePath);
    const inside = relative(this.root, absolute);
    if (inside.startsWith("..") || isAbsolute(inside)) return undefined;

    try {
      const text = await readFile(absolute, "utf8");
      const lines = text.split(/\r?\n/);
      if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
      return lines;
    } catch {
      return undefined;
    }
  }
}
