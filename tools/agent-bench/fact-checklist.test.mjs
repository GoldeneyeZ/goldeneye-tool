import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { answerText, evaluateChecklist } from "./graders/fact-checklist.mjs";

const GRADER = join(dirname(fileURLToPath(import.meta.url)), "graders", "fact-checklist.mjs");

const CHECKLIST = {
  facts: [
    { id: "entry-point", all: ["DataBinder"], any: ["doBind", "applyPropertyValues"] },
    { id: "error-type", all: ["FieldError"] },
    { id: "resolver", any: ["MessageCodesResolver"] },
  ],
  forbidden: [{ id: "invented-class", any: ["BindingManager"] }],
  min_facts: 2,
};

test("a fact needs every `all` pattern and one `any` pattern", () => {
  const result = evaluateChecklist(CHECKLIST, "DataBinder.doBind records a fielderror.");
  assert.deepEqual(result.missing, ["resolver"]);
  assert.equal(result.found, 2);
  assert.equal(result.passed, true);

  const partial = evaluateChecklist(CHECKLIST, "DataBinder records a FieldError.");
  assert.deepEqual(partial.missing, ["entry-point", "resolver"]);
  assert.equal(partial.passed, false);
});

test("a forbidden pattern or an empty answer fails the checklist", () => {
  const invented = evaluateChecklist(
    CHECKLIST,
    "DataBinder.doBind creates a FieldError through BindingManager.",
  );
  assert.deepEqual(invented.forbidden, ["invented-class"]);
  assert.equal(invented.passed, false);

  assert.equal(evaluateChecklist({ facts: [] }, "  ").passed, false);
  assert.equal(evaluateChecklist({ facts: [] }, "anything").passed, true);
});

test("the final message is unwrapped from its JSON envelope", () => {
  assert.equal(answerText('{"summary":"DataBinder.doBind","tests":[]}'), "DataBinder.doBind");
  assert.equal(answerText("# Binding\nplain text"), "# Binding\nplain text");
});

test("the grader reads the final message or a worktree file and sets the exit code", () => {
  const root = mkdtempSync(join(tmpdir(), "fact-checklist-"));
  try {
    const runDir = join(root, "run");
    const worktree = join(root, "worktree");
    mkdirSync(runDir);
    mkdirSync(join(worktree, "docs"), { recursive: true });
    writeFileSync(
      join(runDir, "final-message.txt"),
      JSON.stringify({ summary: "DataBinder.doBind adds a FieldError.", tests: [] }),
    );
    writeFileSync(join(worktree, "docs", "binding.md"), "Only DataBinder is described here.");
    const run = (checklist) => {
      const path = join(root, "checklist.json");
      writeFileSync(path, JSON.stringify(checklist));
      return spawnSync(
        process.execPath,
        [GRADER, "--checklist", path, "--run-dir", runDir, "--worktree", worktree],
        { encoding: "utf8" },
      );
    };

    const answer = run(CHECKLIST);
    assert.equal(answer.status, 0, answer.stderr);
    assert.equal(JSON.parse(answer.stdout).found, 2);

    const document = run({ ...CHECKLIST, source: "docs/binding.md" });
    assert.equal(document.status, 1);
    assert.deepEqual(JSON.parse(document.stdout).missing, ["entry-point", "error-type", "resolver"]);

    const absent = run({ ...CHECKLIST, source: "docs/missing.md" });
    assert.equal(absent.status, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
