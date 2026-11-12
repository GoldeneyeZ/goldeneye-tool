import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import {
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
    expect(goldeneyeGcalSkillMd).toContain("Stop discovery once the task has enough evidence");
    expect(goldeneyeGcalSkillMd).toContain("Do not run `gcal status` or `gcal --help` unless blocked");
    expect(goldeneyeGcalSkillMd).toContain("chain different");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --file <path>");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --js <code>");
    expect(goldeneyeGcalSkillMd).toContain("true async JavaScript body");
    expect(goldeneyeGcalSkillMd).toContain("loop, branch");
    expect(goldeneyeGcalSkillMd).toContain("`gcal.search`, `gcal.select`, `gcal.source`");
    expect(goldeneyeGcalSkillMd).toContain("`gcal.trySource`");
    expect(goldeneyeGcalSkillMd).toContain("Return a JSON-serializable value");
    expect(goldeneyeGcalSkillMd).toContain("not a security sandbox");
  });

  it("documents every discovery command and locator form", () => {
    for (const command of ["get", "search", "callers", "callees", "impls", "grep", "arch"]) {
      expect(goldeneyeGcalSkillMd).toContain(`\`gcal ${command}`);
    }
    expect(goldeneyeGcalSkillMd).toContain("`Billing.java:42`");
    expect(goldeneyeGcalSkillMd).toContain("`Billing.java:10-60`");
    expect(goldeneyeGcalSkillMd).toContain("a member outline above 500 lines");
    expect(goldeneyeGcalSkillMd).toContain("-C 8");
    expect(goldeneyeGcalSkillMd).toContain("call graph plus text references");
  });

  it("explains project selection, index freshness, and the daemon", () => {
    expect(goldeneyeGcalSkillMd).toContain("Run `gcal init` once from the project root");
    expect(goldeneyeGcalSkillMd).toContain("$HOME/.gcal/projects.json");
    expect(goldeneyeGcalSkillMd).toContain("most specific registered ancestor");
    expect(goldeneyeGcalSkillMd).toContain("an optional explicit override");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_HOME`");
    expect(goldeneyeGcalSkillMd).toContain("`gcal index` only indexes");
    expect(goldeneyeGcalSkillMd).toContain("reads the working tree");
    expect(goldeneyeGcalSkillMd).toContain("on-demand local GCAL daemon");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_DAEMON_IDLE=10m`");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_DAEMON=off`");
    expect(goldeneyeGcalSkillMd).toContain("always bypasses the daemon");
  });
});
