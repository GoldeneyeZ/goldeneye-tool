import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();

describe("PowerShell installer", () => {
  it("installs GCAL globally and publishes the Codex skill assets", async () => {
    const script = await readFile(join(root, "install.ps1"), "utf8");

    expect(script).toContain("[switch]$SkipBuild");
    expect(script).toContain("[switch]$SkipGlobalLink");
    expect(script).toContain("[switch]$SkipSkills");
    expect(script).toContain("[string]$GoldeneyeCommand");
    expect(script).toContain("[switch]$Force");
    expect(script).toContain("pnpm install");
    expect(script).toContain("pnpm build");
    expect(script).toContain("pnpm add --global --allow-build=esbuild");
    expect(script).toContain('"GCAL_BACKEND", "goldeneye", "User"');
    expect(script).toContain('"GCAL_GOLDENEYE_COMMAND", $resolvedGoldeneyeCommand, "User"');
    expect(script).toContain('"GCAL_MCP_COMMAND", $null, "User"');
    expect(script).toContain('"GCAL_MCP_URL", $null, "User"');
    expect(script).toContain("$LASTEXITCODE");
    expect(script).toContain("failed with exit code");
    expect(script).toContain("workflow/skills/goldeneye-code-agent-layer");
    expect(script).toContain(".codex/skills/goldeneye-code-agent-layer");
    expect(script).toContain('Join-Path $skillsRoot "codebase-memory"');
    expect(script).toContain("Test-LegacyGcalSkill");
    expect(script).toContain("<!-- gcal-installer:start -->");
    expect(script).toContain("<!-- gcal-installer:end -->");
    expect(script).toContain("<!-- codebase-memory-mcp:start -->");
    expect(script).toContain("<!-- codebase-memory-mcp:end -->");
    expect(script).toContain("Remove-ManagedBlock");
  });

  it("documents the installer command and verification step", async () => {
    const readme = await readFile(join(root, "README.md"), "utf8");

    expect(readme).toContain(".\\agent\\install.ps1");
    expect(readme).toContain("gcal --help");
    expect(readme).toContain("$HOME\\.codex\\skills\\goldeneye-code-agent-layer");
    expect(readme).toContain("Goldeneye is always selected when `GCAL_BACKEND` is unset");
    expect(readme.toLowerCase()).not.toContain("codebase-memory");
    expect(readme).toContain("sole model-facing code-discovery surface");
    expect(readme).toContain("defaults to 5 candidates");
    expect(readme).toContain("defaults to depth 1 and 20 rows");
    expect(readme).toContain("omits the full file tree");
  });
});
