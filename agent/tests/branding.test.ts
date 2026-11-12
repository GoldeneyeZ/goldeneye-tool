import { readFile, readdir } from "node:fs/promises";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();
const retiredBrand = /codebase(?:[-_\s]?memory)/i;

async function sourceFiles(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return sourceFiles(path);
      }
      return entry.isFile() && entry.name.endsWith(".ts") ? [path] : [];
    }),
  );
  return files.flat();
}

describe("Goldeneye branding", () => {
  it("keeps active user-facing documentation on Goldeneye terminology", async () => {
    const paths = [
      "README.md",
      "AGENTS.md",
      "workflow/AGENTS.md",
      "workflow/skills/goldeneye-code-agent-layer/SKILL.md",
      "workflow/skills/goldeneye-code-agent-layer/agents/claude.md",
      "workflow/skills/goldeneye-code-agent-layer/agents/openai.yaml",
    ];

    for (const path of paths) {
      expect(await readFile(join(root, path), "utf8"), path).not.toMatch(retiredBrand);
    }
  });

  it("keeps retired backend branding inside adapters only", async () => {
    const srcRoot = join(root, "src");
    const files = (await sourceFiles(srcRoot)).filter(
      (path) => !relative(srcRoot, path).startsWith("adapters"),
    );

    for (const path of files) {
      expect(await readFile(path, "utf8"), relative(srcRoot, path)).not.toMatch(retiredBrand);
    }
  });
});
