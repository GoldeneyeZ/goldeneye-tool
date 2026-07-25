import assert from "node:assert/strict";
import test from "node:test";

import {
  LIMITATIONS,
  auditBenchmarkReport,
  mergeReportRuns,
  renderMarkdownReport,
} from "./report.mjs";

function scoredRun(id, engine, repetition) {
  return {
    id,
    engine,
    repetition,
    success: true,
    grader_exit_code: 0,
    wall_ms: 100,
    grader_ms: 25,
    completion_ms: 100,
    verified_e2e_ms: 125,
    dirty_paths: 2,
    patch_files: 2,
    snapshot: engine === "goldeneye-ack" ? { manifest_sha256: "snapshot" } : null,
    pre_run_verification: {
      candidate_unchanged: true,
      source_repository_clean: true,
      snapshot: { manifest_sha256: "snapshot" },
    },
    artifact_dir: `artifacts/${id}`,
  };
}

test("mergeReportRuns appends split lanes and rejects duplicate run IDs", () => {
  const merged = mergeReportRuns(
    [scoredRun("vanilla-1", "vanilla", 1)],
    [scoredRun("ack-1", "goldeneye-ack", 1)],
  );
  assert.deepEqual(merged.map((run) => run.id), ["vanilla-1", "ack-1"]);
  assert.throws(() => mergeReportRuns(merged, [merged[0]]), /Duplicate scored run ID/);
});

test("renderMarkdownReport includes descriptive comparison and required limitations", () => {
  const report = {
    runs: [
      scoredRun("vanilla-1", "vanilla", 1),
      scoredRun("ack-1", "goldeneye-ack", 1),
    ],
  };
  const markdown = renderMarkdownReport(report, {
    candidateEngine: "goldeneye-ack",
    vanillaEngine: "vanilla",
  });
  assert.match(markdown, /cached descriptive comparison/i);
  assert.match(markdown, new RegExp(LIMITATIONS.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
});

test("auditBenchmarkReport enforces four traceable scored runs and timing invariants", () => {
  const runs = [
    scoredRun("vanilla-1", "vanilla", 1),
    scoredRun("ack-1", "goldeneye-ack", 1),
    scoredRun("ack-2", "goldeneye-ack", 2),
    scoredRun("ack-3", "goldeneye-ack", 3),
  ];
  const report = { runs };
  const markdown = renderMarkdownReport(report, {
    candidateEngine: "goldeneye-ack",
    vanillaEngine: "vanilla",
  });
  const audit = auditBenchmarkReport(report, {
    allowedDirtyPaths: ["Main.java", "MainTests.java"],
    artifactExists: () => true,
    candidateEngine: "goldeneye-ack",
    markdown,
    readArtifact: () => " M Main.java\n M MainTests.java\n",
    vanillaEngine: "vanilla",
  });
  assert.equal(audit.passed, true);
  assert.equal(audit.run_count, 4);
  assert.throws(
    () =>
      auditBenchmarkReport({ runs: runs.slice(1) }, {
        allowedDirtyPaths: ["Main.java", "MainTests.java"],
        artifactExists: () => true,
        candidateEngine: "goldeneye-ack",
        markdown,
        readArtifact: () => " M Main.java\n M MainTests.java\n",
        vanillaEngine: "vanilla",
      }),
    /expected 4 scored runs/,
  );
});
