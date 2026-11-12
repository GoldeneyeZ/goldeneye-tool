import { compileDirtyPathPolicy, evaluateDirtyPaths } from "./path-policy.mjs";

const REQUIRED_ARTIFACTS = [
  "codex.jsonl",
  "grader.stdout.log",
  "patch.diff",
  "prompt.txt",
  "status.txt",
];

export function mergeReportRuns(existing, additions) {
  const ids = new Set(existing.map((run) => run.id));
  for (const run of additions) {
    if (ids.has(run.id)) throw new Error(`Duplicate scored run ID: ${run.id}`);
    ids.add(run.id);
  }
  return [...existing, ...additions];
}

export function renderLimitations({ candidateCount, vanillaCount, randomized }) {
  return `This benchmark contains ${candidateCount} candidate and ${vanillaCount} vanilla ` +
    `${randomized ? "randomized serial" : "serial"} runs. Results are descriptive; ` +
    "the sample is too small for inferential significance. Provider prefix caching is " +
    "reported separately from GCAL snapshot caching.";
}

export function renderMarkdownReport(report, { candidateEngine, vanillaEngine }) {
  const runs = report.runs ?? [];
  const candidate = runs.filter((run) => run.engine === candidateEngine);
  const vanilla = runs.filter((run) => run.engine === vanillaEngine);
  const randomized = isRandomized(report);
  const lines = [
    "# Spring StringUtils Unicode Truncation Benchmark",
    "",
    "## Scored runs",
    "",
    "| Run | Engine | Rep | Correct | Wall ms | Grader ms | Tokens | GCAL calls | Failed GCAL | Patch files | Artifact |",
    "| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
  ];
  for (const run of runs) {
    lines.push(
      `| ${run.id} | ${run.engine} | ${run.repetition} | ${run.success ? "PASS" : "FAIL"} | ${number(run.wall_ms)} | ${number(run.grader_ms)} | ${number(run.total_tokens)} | ${number(run.gcal_calls)} | ${number(run.gcal_failures)} | ${number(run.patch_files)} | \`${run.artifact_dir}\` |`,
    );
  }
  lines.push(
    "",
    "## Candidate summary",
    "",
    `Goldeneye+GCAL: ${candidate.length} serial runs; correctness ${candidate.filter((run) => run.success).length}/${candidate.length}.`,
    metricSummary("wall_ms", candidate),
    metricSummary("total_tokens", candidate),
    metricSummary("gcal_calls", candidate),
    `Raw candidate values remain in \`report.json\`; medians and ranges are descriptive only (n=${candidate.length}).`,
    "",
    "## Vanilla comparison",
    "",
    `Vanilla: ${vanilla.length} cached descriptive comparison ${runNoun(vanilla.length)}. Provenance and artifact paths remain in \`report.json\`.`,
    "",
    "## Limitations",
    "",
    renderLimitations({
      candidateCount: candidate.length,
      vanillaCount: vanilla.length,
      randomized,
    }),
    "",
  );
  return lines.join("\n");
}

export function auditBenchmarkReport(
  report,
  {
    allowedDirtyPaths,
    artifactExists,
    candidateEngine,
    dirtyPathPolicy,
    expectedCandidateRuns = 3,
    expectedVanillaRuns = 1,
    markdown,
    readArtifact,
    vanillaEngine,
  },
) {
  const runs = report.runs ?? [];
  const candidate = runs.filter((run) => run.engine === candidateEngine);
  const vanilla = runs.filter((run) => run.engine === vanillaEngine);
  requireAudit(candidate.length === expectedCandidateRuns,
    `expected ${expectedCandidateRuns} candidate runs, got ${candidate.length}`);
  requireAudit(vanilla.length === expectedVanillaRuns,
    `expected ${expectedVanillaRuns} vanilla runs, got ${vanilla.length}`);
  requireAudit(new Set(runs.map((run) => run.id)).size === runs.length, "run IDs are not unique");
  const effectiveDirtyPathPolicy = dirtyPathPolicy ?? compileDirtyPathPolicy({
    exact: allowedDirtyPaths ?? [],
  });

  for (const run of runs) {
    requireAudit(run.artifact_dir, `run ${run.id} has no artifact directory`);
    for (const name of REQUIRED_ARTIFACTS) {
      requireAudit(
        artifactExists(run.artifact_dir, name),
        `run ${run.id} is missing artifact ${name}`,
      );
    }
    requireAudit(run.completion_ms === run.wall_ms, `run ${run.id} completion_ms differs from wall_ms`);
    requireAudit(
      Math.abs(Math.round(run.verified_e2e_ms) - Math.round(run.wall_ms + run.grader_ms)) <= 1,
      `run ${run.id} verified_e2e_ms is inconsistent`,
    );
    requireAudit(
      run.pre_run_verification?.candidate_unchanged === true,
      `run ${run.id} candidate fingerprint was not verified`,
    );
    requireAudit(
      run.pre_run_verification?.source_repository_clean === true,
      `run ${run.id} source repository was not verified`,
    );
    if (run.success) {
      requireAudit(run.grader_exit_code === 0, `run ${run.id} passed without grader PASS`);
      requireAudit(run.dirty_paths === run.patch_files, `run ${run.id} has unexpected dirty paths`);
      const dirtyPaths = readArtifact(run.artifact_dir, "status.txt")
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => line.slice(3).trim());
      const dirtyPathEvaluation = evaluateDirtyPaths(dirtyPaths, effectiveDirtyPathPolicy);
      requireAudit(dirtyPathEvaluation.passed, `run ${run.id} changed a path outside the allowlist`);
    }
  }

  const hashes = new Set(candidate.map((run) => run.snapshot?.manifest_sha256));
  requireAudit(hashes.size === 1 && !hashes.has(undefined), "candidate snapshot hash differs");
  requireAudit(markdown.includes(renderLimitations({
    candidateCount: candidate.length,
    vanillaCount: vanilla.length,
    randomized: isRandomized(report),
  })), "report limitations text is missing");
  return {
    passed: true,
    run_count: runs.length,
    candidate_count: candidate.length,
    vanilla_count: vanilla.length,
    snapshot_manifest_sha256: [...hashes][0],
  };
}

function isRandomized(report) {
  return (report.settings?.randomized_order?.length ?? 0) > 0;
}

function runNoun(count) {
  return count === 1 ? "run" : "runs";
}

function metricSummary(name, runs) {
  const values = runs.map((run) => run[name]).filter(Number.isFinite).sort((a, b) => a - b);
  if (values.length === 0) return `${name}: no successful values.`;
  const middle = Math.floor(values.length / 2);
  const median = values.length % 2 === 0
    ? (values[middle - 1] + values[middle]) / 2
    : values[middle];
  return `${name}: median ${number(median)}, range ${number(values[0])}–${number(values.at(-1))}; raw ${values.map(number).join(", ")}.`;
}

function number(value) {
  return Number.isFinite(value) ? String(Math.round(value)) : "n/a";
}

function requireAudit(condition, message) {
  if (!condition) throw new Error(`Benchmark report audit failed: ${message}`);
}
