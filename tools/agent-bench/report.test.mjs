import assert from "node:assert/strict";
import test from "node:test";

import { compileDirtyPathPolicy } from "./path-policy.mjs";

import {
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

function fixtureReport({ candidateRuns = 3, vanillaRuns = 1 }) {
  return {
    runs: [
      ...Array.from(
        { length: candidateRuns },
        (_, index) => scoredRun(`ack-${index + 1}`, "goldeneye-ack", index + 1),
      ),
      ...Array.from(
        { length: vanillaRuns },
        (_, index) => scoredRun(`vanilla-${index + 1}`, "vanilla", index + 1),
      ),
    ],
  };
}

function expectedLimitations({ candidateCount, vanillaCount, randomized }) {
  return `This benchmark contains ${candidateCount} candidate and ${vanillaCount} vanilla ` +
    `${randomized ? "randomized serial" : "serial"} runs. Results are descriptive; ` +
    "the sample is too small for inferential significance. Provider prefix caching is " +
    "reported separately from ACK snapshot caching.";
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
  assert.match(markdown, new RegExp(expectedLimitations({
    candidateCount: 1,
    vanillaCount: 1,
    randomized: false,
  }).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
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
    dirtyPathPolicy: compileDirtyPathPolicy({ exact: ["Main.java", "MainTests.java"] }),
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
        dirtyPathPolicy: compileDirtyPathPolicy({ exact: ["Main.java", "MainTests.java"] }),
        artifactExists: () => true,
        candidateEngine: "goldeneye-ack",
        markdown,
        readArtifact: () => " M Main.java\n M MainTests.java\n",
        vanillaEngine: "vanilla",
      }),
    /expected 1 vanilla run/,
  );
});

test("audits a randomized three by three report", () => {
  const report = fixtureReport({
    candidateRuns: 3,
    vanillaRuns: 3,
  });
  const audit = auditBenchmarkReport(report, {
    expectedCandidateRuns: 3,
    expectedVanillaRuns: 3,
    dirtyPathPolicy: compileDirtyPathPolicy({ prefixes: ["spring-context/"] }),
    artifactExists: () => true,
    candidateEngine: "goldeneye-ack",
    markdown: renderMarkdownReport(report, {
      candidateEngine: "goldeneye-ack",
      vanillaEngine: "vanilla",
    }),
    readArtifact: () => " M spring-context/src/main/java/A.java\n",
    vanillaEngine: "vanilla",
  });
  assert.equal(audit.passed, true);
  assert.equal(audit.run_count, 6);
  assert.equal(audit.vanilla_count, 3);
});
