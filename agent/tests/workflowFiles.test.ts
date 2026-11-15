import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import {
  goldeneyeGcalReferenceMd,
  goldeneyeGcalSkillMd,
  workflowAgentsMd,
  workflowFiles,
} from "../src/workflows/createWorkflowFiles.js";

describe("workflow files", () => {
  it("keeps committed assets aligned with generated assets", async () => {
    for (const file of workflowFiles) {
      const committed = (await readFile(file.path, "utf8")).replace(/\r\n/g, "\n");
      expect(committed).toBe(file.content);
    }
  });

  it("routes each discovery need through one surface and one call", () => {
    expect(workflowAgentsMd).toContain("sole model-facing code-discovery surface");
    expect(workflowAgentsMd).toContain("never repeat the same query through both surfaces");
    expect(workflowAgentsMd).toContain("never search only to learn an identifier");
    expect(goldeneyeGcalSkillMd).toContain("name: goldeneye-code-agent-layer");
    expect(goldeneyeGcalSkillMd).toContain("One `gcal` call should answer one question");
    expect(goldeneyeGcalSkillMd).toContain("Never search only to learn an");
    expect(goldeneyeGcalSkillMd).toContain("Never ask the same question through both");
    expect(goldeneyeGcalSkillMd).toContain("Stop discovery once the task has enough evidence");
    expect(goldeneyeGcalSkillMd).toContain("Do not run `gcal status` or `gcal --help` unless blocked");
    expect(goldeneyeGcalSkillMd).toContain("chain commands in one shell call");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --file <path>");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --js <code>");
  });

  it("stays short: it is sent with every model call", () => {
    // 10,356 bytes before 2026-11-15; the rest moved to reference.md and to
    // the output of the commands.
    expect(Buffer.byteLength(goldeneyeGcalSkillMd)).toBeLessThan(3600);
    expect(goldeneyeGcalSkillMd).toContain("`reference.md`");
  });

  it("documents every discovery command and locator form", () => {
    for (const command of ["get", "search", "callers", "callees", "impls", "grep", "replace", "check", "arch"]) {
      expect(goldeneyeGcalSkillMd).toContain(`\`gcal ${command}`);
    }
    expect(goldeneyeGcalSkillMd).toContain("`Billing.java:42`");
    expect(goldeneyeGcalSkillMd).toContain("`Billing.java:10-60`");
    expect(goldeneyeGcalSkillMd).toContain("a member outline above 500 lines");
    expect(goldeneyeGcalSkillMd).toContain("-C 8");
    expect(goldeneyeGcalSkillMd).toContain("call graph plus text references");
    expect(goldeneyeGcalSkillMd).toContain("after edits in more than one file");
  });

  it("keeps the workflow API in the reference", () => {
    expect(goldeneyeGcalReferenceMd).toContain("true async JavaScript body");
    expect(goldeneyeGcalReferenceMd).toContain("loop, branch");
    expect(goldeneyeGcalReferenceMd).toContain("`gcal.search`, `gcal.select`, `gcal.source`");
    expect(goldeneyeGcalReferenceMd).toContain("`gcal.trySource`");
    expect(goldeneyeGcalReferenceMd).toContain("Return a JSON-serializable value");
    expect(goldeneyeGcalReferenceMd).toContain("not a security sandbox");
  });

  it("explains project selection, index freshness, and the daemon in the reference", () => {
    expect(goldeneyeGcalSkillMd).toContain("run `gcal init` once from the project");
    expect(goldeneyeGcalReferenceMd).toContain("Run `gcal init` once from the project root");
    expect(goldeneyeGcalReferenceMd).toContain("$HOME/.gcal/projects.json");
    expect(goldeneyeGcalReferenceMd).toContain("most specific registered ancestor");
    expect(goldeneyeGcalReferenceMd).toContain("an optional explicit override");
    expect(goldeneyeGcalReferenceMd).toContain("`GCAL_HOME`");
    expect(goldeneyeGcalReferenceMd).toContain("`gcal index` only indexes");
    expect(goldeneyeGcalReferenceMd).toContain("reads the working tree");
    expect(goldeneyeGcalReferenceMd).toContain("on-demand local GCAL daemon");
    expect(goldeneyeGcalReferenceMd).toContain("`GCAL_DAEMON_IDLE=10m`");
    expect(goldeneyeGcalReferenceMd).toContain("`GCAL_DAEMON=off`");
    expect(goldeneyeGcalReferenceMd).toContain("always bypasses the daemon");
  });
});
