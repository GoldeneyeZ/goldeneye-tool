#!/usr/bin/env node

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { isDirectSourceReadCommand, summarizeRuns } from "./core.mjs";

const args = process.argv.slice(2);
const outIndex = args.indexOf("--out");
if (outIndex < 0 || !args[outIndex + 1]) {
  fail("Usage: node tools/agent-bench/merge-reports.mjs --out <report.json> <input.json>...");
}
const output = resolve(args[outIndex + 1]);
const inputs = args
  .filter((_, index) => index !== outIndex && index !== outIndex + 1)
  .map((value) => resolve(value));
if (inputs.length < 2) fail("At least two input reports are required");

const reports = inputs.map((path) => ({ path, report: JSON.parse(readFileSync(path, "utf8")) }));
const reference = reports[0].report;
for (const { path, report } of reports.slice(1)) {
  requireEqual(report.repository, reference.repository, "repository", path);
  requireEqual(report.base_commit, reference.base_commit, "base_commit", path);
  requireEqual(report.settings?.model, reference.settings?.model, "settings.model", path);
  requireEqual(report.settings?.reasoning, reference.settings?.reasoning, "settings.reasoning", path);
}

const runs = [];
const runIds = new Set();
for (const { path, report } of reports) {
  for (const run of report.runs ?? []) {
    if (runIds.has(run.id)) fail(`Duplicate run id ${run.id} from ${path}`);
    runIds.add(run.id);
    const protocolViolations = (run.protocol_violations ?? []).filter(
      (violation) =>
        violation.type !== "direct_source_read" || isDirectSourceReadCommand(violation.command),
    );
    const success =
      run.codex_exit_code === 0 &&
      !run.timed_out &&
      run.grader_exit_code === 0 &&
      protocolViolations.length === 0;
    runs.push({
      ...run,
      raw_success: run.success,
      success,
      protocol_violations: protocolViolations,
      source_report: basename(path),
    });
  }
}

const merged = {
  generated_at: new Date().toISOString(),
  kind: "merged-agent-benchmark",
  sources: inputs,
  repository: reference.repository,
  repository_name: reference.repository_name,
  base_commit: reference.base_commit,
  settings: reference.settings,
  runs,
  summary: summarizeRuns(runs),
};
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify(merged, null, 2)}\n`);
console.log(`Merged ${runs.length} runs into ${output}`);

function requireEqual(actual, expected, field, path) {
  if (actual !== expected) {
    fail(`Incompatible ${field} in ${path}: expected ${expected}, got ${actual}`);
  }
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
