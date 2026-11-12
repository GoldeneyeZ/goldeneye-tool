#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import {
  DEFAULT_COST_MODEL,
  createShellExecutor,
  expandPlaceholders,
  prepareGcalShim,
  renderMarkdown,
  resolveProjectName,
  runChain,
  summarize,
} from '../ops.mjs';

const USAGE = `Usage: benchmark-ops.mjs --scenarios <file> --repo <dir> --state <dir>
         --gcal-entry <dist/main.js> --goldeneye <binary> [options]

Options:
  --out <file>        JSON report path (default: <state>/report-<label>.json)
  --label <name>      report label (default: run)
  --lanes <a,b>       lanes to run (default: every lane in the scenario file)
  --only <id,id>      run only these scenario ids
  --rg-dir <dir>      directory holding rg, prepended to PATH for the vanilla lane
  --verbose           print each executed step
`;

function parseArgs(argv) {
  const options = { label: 'run', verbose: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help') {
      process.stdout.write(USAGE);
      process.exit(0);
    }
    if (arg === '--verbose') {
      options.verbose = true;
      continue;
    }
    if (!arg.startsWith('--') || index + 1 >= argv.length) {
      throw new Error(`unexpected argument: ${arg}`);
    }
    options[arg.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = argv[index + 1];
    index += 1;
  }
  for (const required of ['scenarios', 'repo', 'state', 'gcalEntry', 'goldeneye']) {
    if (!options[required]) throw new Error(`missing --${required.replace(/[A-Z]/g, (l) => `-${l.toLowerCase()}`)}`);
  }
  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const suite = JSON.parse(readFileSync(options.scenarios, 'utf8'));
  const repo = path.resolve(options.repo);
  const state = path.resolve(options.state);
  const gcalHome = path.join(state, 'gcal-home');
  const shimDir = prepareGcalShim(state, path.resolve(options.gcalEntry));
  const lanes = options.lanes ? options.lanes.split(',') : suite.lanes;
  const only = options.only ? new Set(options.only.split(',')) : null;

  const env = { ...process.env, ...suite.env };
  delete env.GCAL_PROJECT;
  Object.assign(env, {
    GCAL_HOME: gcalHome,
    GCAL_BACKEND: 'goldeneye',
    GCAL_GOLDENEYE_COMMAND: path.resolve(options.goldeneye),
    GCAL_DAEMON: 'auto',
    GCAL_DAEMON_IDLE: '60s',
    GOLDENEYE_DB_PATH: path.join(state, 'goldeneye.db'),
    PATH: [shimDir, options.rgDir, process.env.PATH].filter(Boolean).join(path.delimiter),
  });

  const execute = createShellExecutor({ cwd: repo, env });
  const placeholders = { P: resolveProjectName(gcalHome, repo) };
  for (const [key, template] of Object.entries(suite.placeholders ?? {})) {
    placeholders[key] = expandPlaceholders(template, placeholders);
  }

  // Start the GCAL daemon outside the measured chains; report its cold cost once.
  const warmup = execute('gcal status');
  if (warmup.exitCode !== 0) {
    throw new Error(`gcal warm-up failed: ${warmup.output.trim()}`);
  }

  const scenarios = [];
  for (const scenario of suite.scenarios) {
    if (only && !only.has(scenario.id)) continue;
    const laneResults = {};
    for (const lane of lanes) {
      const steps = scenario[lane];
      if (!Array.isArray(steps)) continue;
      laneResults[lane] = runChain({
        steps,
        evidence: scenario.evidence,
        execute,
        placeholders,
        costModel: DEFAULT_COST_MODEL,
      });
      if (options.verbose) {
        for (const step of laneResults[lane].steps) {
          process.stderr.write(
            `[${scenario.id}/${lane}] x=${step.exit_code} ${step.ms}ms ${step.bytes}B ${step.command}\n`,
          );
        }
        if (!laneResults[lane].ok) {
          process.stderr.write(
            `[${scenario.id}/${lane}] MISSING ${laneResults[lane].missing_evidence.join(' | ')}\n`,
          );
        }
      }
    }
    scenarios.push({
      id: scenario.id,
      category: scenario.category,
      need: scenario.need,
      lanes: laneResults,
    });
  }

  const report = {
    schema_version: 1,
    kind: 'ops-benchmark',
    generated_at: new Date().toISOString(),
    label: options.label,
    corpus: suite.corpus,
    repo,
    gcal_entry: path.resolve(options.gcalEntry),
    goldeneye: path.resolve(options.goldeneye),
    lanes,
    cost_model: DEFAULT_COST_MODEL,
    daemon_cold_ms: Math.round(warmup.ms),
    scenarios,
    summary: summarize(scenarios, lanes),
  };

  const out = path.resolve(options.out ?? path.join(state, `report-${options.label}.json`));
  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`);
  const markdown = renderMarkdown(report);
  writeFileSync(out.replace(/\.json$/, '.md'), markdown);
  process.stdout.write(markdown);
  process.stdout.write(`\ndaemon cold start: ${report.daemon_cold_ms} ms\nreport: ${out}\n`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`error: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
}
