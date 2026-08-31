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

  it("routes each discovery need through one surface and one command path", () => {
    expect(workflowAgentsMd).toContain("sole model-facing code-discovery surface");
    expect(workflowAgentsMd).toContain("never repeat the same query through both surfaces");
    expect(goldeneyeGcalSkillMd).toContain("name: goldeneye-code-agent-layer");
    expect(goldeneyeGcalSkillMd).toContain("Exact qualified name and source needed");
    expect(goldeneyeGcalSkillMd).toContain("Do not run `search` before `inspect`");
    expect(goldeneyeGcalSkillMd).toContain("Stop discovery once the task has enough evidence");
    expect(goldeneyeGcalSkillMd).toContain("Run one `gcal get <qualified-name...>`");
    expect(goldeneyeGcalSkillMd).toContain("--query <query-2> ... --snippets <n>");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --file <path>");
    expect(goldeneyeGcalSkillMd).toContain("gcal workflow --js <code>");
    expect(goldeneyeGcalSkillMd).toContain("true async JavaScript body");
    expect(goldeneyeGcalSkillMd).toContain("loop, branch");
    expect(goldeneyeGcalSkillMd).toContain("`gcal.search`, `gcal.select`, `gcal.source`");
    expect(goldeneyeGcalSkillMd).toContain("`gcal.trySource`");
    expect(goldeneyeGcalSkillMd).toContain("Return a JSON-serializable value");
    expect(goldeneyeGcalSkillMd).toContain("not a security sandbox");
    expect(goldeneyeGcalSkillMd).toContain("Keep batches at five items or fewer");
    expect(goldeneyeGcalSkillMd).toContain("Exceed five only when every item is");
    expect(goldeneyeGcalSkillMd).toContain("directly relevant to the current task");
    expect(goldeneyeGcalSkillMd).toContain(
      "A normal unknown-source lookup uses one `gcal search --snippets` invocation",
    );
    expect(goldeneyeGcalSkillMd).toContain(
      "Do not run `gcal status` or `gcal --help` unless blocked",
    );
    expect(goldeneyeGcalSkillMd).toContain("Run `gcal init` once from the project root");
    expect(goldeneyeGcalSkillMd).toContain("$HOME/.gcal/projects.json");
    expect(goldeneyeGcalSkillMd).toContain("most specific registered ancestor");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_PROJECT` is an optional explicit override");
    expect(goldeneyeGcalSkillMd).toContain(
      "Set `GCAL_HOME` to an isolated absolute state directory",
    );
    expect(goldeneyeGcalSkillMd).toContain("`gcal index` only indexes");
    expect(goldeneyeGcalSkillMd).toContain("on-demand local GCAL daemon");
    expect(goldeneyeGcalSkillMd).toContain("one backend");
    expect(goldeneyeGcalSkillMd).toContain("session per active project");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_DAEMON_IDLE=10m`");
    expect(goldeneyeGcalSkillMd).toContain("`GCAL_DAEMON=off`");
    expect(goldeneyeGcalSkillMd).toContain("Benchmark mode always bypasses the daemon");
  });
});
