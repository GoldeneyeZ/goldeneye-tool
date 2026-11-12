// Deterministic operation-level benchmark: no model in the loop.
//
// Each scenario states one information need, the evidence that proves the
// need was met, and one scripted command chain per lane. A lane stops at the
// first step whose cumulative output contains all evidence, mirroring an agent
// that stops discovery once it has enough.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';

export const DEFAULT_COST_MODEL = Object.freeze({
  // Context a model re-reads on every additional turn (tokens).
  baseContextTokens: 25_000,
  // Later turns of the same session that re-read a tool output.
  carryTurns: 10,
  // Price of a cached input token relative to an uncached one.
  cachedPriceRatio: 0.1,
  charsPerToken: 4,
});

export function estimateTokens(text, model = DEFAULT_COST_MODEL) {
  return Math.ceil(text.length / model.charsPerToken);
}

// Uncached-token-equivalent cost of a chain of tool calls.
// Every call adds one model turn that re-reads the cached base context; every
// output token is paid once uncached and then re-read cached on later turns.
export function chainCost(outputTokens, model = DEFAULT_COST_MODEL) {
  const turnCost = outputTokens.length * model.baseContextTokens * model.cachedPriceRatio;
  const carryFactor = 1 + model.carryTurns * model.cachedPriceRatio;
  const outputCost = outputTokens.reduce((sum, tokens) => sum + tokens, 0) * carryFactor;
  return Math.round(turnCost + outputCost);
}

export function compileEvidence(evidence) {
  return evidence.map((item) => {
    if (typeof item === 'string') {
      return { label: item, test: (text) => text.includes(item) };
    }
    if (item && typeof item.regex === 'string') {
      const pattern = new RegExp(item.regex, item.flags ?? '');
      return { label: `/${item.regex}/`, test: (text) => pattern.test(text) };
    }
    throw new Error(`invalid evidence entry: ${JSON.stringify(item)}`);
  });
}

export function missingEvidence(compiled, text) {
  return compiled.filter((item) => !item.test(text)).map((item) => item.label);
}

export function expandPlaceholders(command, values) {
  return command.replace(/\$\{([A-Za-z_]+)\}/g, (match, key) => {
    if (!(key in values)) {
      throw new Error(`unknown placeholder ${match} in: ${command}`);
    }
    return values[key];
  });
}

export function runChain({ steps, evidence, execute, placeholders = {}, costModel }) {
  const compiled = compileEvidence(evidence);
  const executed = [];
  let cumulative = '';
  let missing = missingEvidence(compiled, cumulative);

  for (const step of steps) {
    // A recovery step models what an agent does only once something went
    // wrong; it is skipped while every earlier command exited cleanly.
    const recovery = typeof step === 'object' && step.when === 'after-failure';
    if (recovery && executed.every((earlier) => earlier.exit_code === 0)) continue;
    const template = typeof step === 'string' ? step : step.cmd;
    const command = expandPlaceholders(template, placeholders);
    const result = execute(command);
    cumulative += `${result.output}\n`;
    executed.push({
      command,
      exit_code: result.exitCode,
      ms: Math.round(result.ms),
      bytes: Buffer.byteLength(result.output, 'utf8'),
      tokens: estimateTokens(result.output, costModel),
    });
    missing = missingEvidence(compiled, cumulative);
    if (missing.length === 0) break;
  }

  const tokens = executed.map((step) => step.tokens);
  return {
    ok: missing.length === 0,
    missing_evidence: missing,
    calls: executed.length,
    failed_calls: executed.filter((step) => step.exit_code !== 0).length,
    bytes: executed.reduce((sum, step) => sum + step.bytes, 0),
    tokens: tokens.reduce((sum, value) => sum + value, 0),
    ms: executed.reduce((sum, step) => sum + step.ms, 0),
    cost: chainCost(tokens, costModel),
    steps: executed,
  };
}

export function summarize(results, lanes) {
  const totals = Object.fromEntries(
    lanes.map((lane) => [
      lane,
      { scenarios: 0, ok: 0, calls: 0, failed_calls: 0, bytes: 0, tokens: 0, ms: 0, cost: 0 },
    ]),
  );
  const byCategory = {};

  for (const scenario of results) {
    const category = (byCategory[scenario.category] ??= Object.fromEntries(
      lanes.map((lane) => [lane, { scenarios: 0, ok: 0, calls: 0, tokens: 0, ms: 0, cost: 0 }]),
    ));
    for (const lane of lanes) {
      const run = scenario.lanes[lane];
      if (run === undefined) continue;
      const total = totals[lane];
      total.scenarios += 1;
      total.ok += run.ok ? 1 : 0;
      total.calls += run.calls;
      total.failed_calls += run.failed_calls;
      total.bytes += run.bytes;
      total.tokens += run.tokens;
      total.ms += run.ms;
      total.cost += run.cost;
      const bucket = category[lane];
      bucket.scenarios += 1;
      bucket.ok += run.ok ? 1 : 0;
      bucket.calls += run.calls;
      bucket.tokens += run.tokens;
      bucket.ms += run.ms;
      bucket.cost += run.cost;
    }
  }

  return { totals, by_category: byCategory };
}

export function renderMarkdown(report) {
  const lanes = report.lanes;
  const cell = (run) =>
    run === undefined
      ? 'n/a'
      : `${run.ok ? 'ok' : 'FAIL'} ${run.calls}c ${run.tokens}t ${run.ms}ms ${run.cost}u`;
  const lines = [
    `# Ops benchmark: ${report.corpus} (${report.label})`,
    '',
    `Cost unit (u) = uncached-token equivalent: calls x ${report.cost_model.baseContextTokens} x ${report.cost_model.cachedPriceRatio} + output tokens x (1 + ${report.cost_model.carryTurns} x ${report.cost_model.cachedPriceRatio}).`,
    '',
    `| scenario | category | ${lanes.join(' | ')} | winner |`,
    `|---|---|${lanes.map(() => '---').join('|')}|---|`,
  ];

  for (const scenario of report.scenarios) {
    lines.push(
      `| ${scenario.id} | ${scenario.category} | ${lanes
        .map((lane) => cell(scenario.lanes[lane]))
        .join(' | ')} | ${winner(scenario, lanes)} |`,
    );
  }

  lines.push('', '## Totals', '', '| lane | ok | calls | failed calls | tokens | ms | cost |', '|---|---|---|---|---|---|---|');
  for (const lane of lanes) {
    const total = report.summary.totals[lane];
    lines.push(
      `| ${lane} | ${total.ok}/${total.scenarios} | ${total.calls} | ${total.failed_calls} | ${total.tokens} | ${total.ms} | ${total.cost} |`,
    );
  }

  lines.push('', '## By category (cost)', '', `| category | ${lanes.join(' | ')} |`, `|---|${lanes.map(() => '---').join('|')}|`);
  for (const [category, buckets] of Object.entries(report.summary.by_category)) {
    lines.push(
      `| ${category} | ${lanes
        .map((lane) => `${buckets[lane].ok}/${buckets[lane].scenarios} ok, ${buckets[lane].calls}c, ${buckets[lane].cost}u`)
        .join(' | ')} |`,
    );
  }

  return `${lines.join('\n')}\n`;
}

function winner(scenario, lanes) {
  const ranked = lanes
    .filter((lane) => scenario.lanes[lane]?.ok)
    .sort((left, right) => scenario.lanes[left].cost - scenario.lanes[right].cost);
  if (ranked.length === 0) return 'none';
  if (ranked.length === 1) return ranked[0];
  const [best, second] = ranked;
  const ratio = scenario.lanes[second].cost / Math.max(1, scenario.lanes[best].cost);
  return ratio < 1.1 ? 'tie' : `${best} x${ratio.toFixed(1)}`;
}

export function createShellExecutor({ cwd, env, shell = 'pwsh', timeoutMs = 120_000 }) {
  return (command) => {
    const started = performance.now();
    const child = spawnSync(shell, ['-NoProfile', '-NonInteractive', '-Command', command], {
      cwd,
      env,
      encoding: 'utf8',
      timeout: timeoutMs,
      maxBuffer: 64 * 1024 * 1024,
      windowsHide: true,
    });
    const ms = performance.now() - started;
    if (child.error) {
      return { output: `spawn error: ${child.error.message}`, exitCode: -1, ms };
    }
    return {
      output: `${child.stdout ?? ''}${child.stderr ?? ''}`,
      exitCode: child.status ?? -1,
      ms,
    };
  };
}

export function prepareAckShim(stateDir, ackEntry) {
  const binDir = path.join(stateDir, 'bin');
  mkdirSync(binDir, { recursive: true });
  // A PowerShell shim, not a .cmd one: cmd.exe would reinterpret `|`, `^` and
  // `&` inside quoted regex arguments before node ever sees them.
  rmSync(path.join(binDir, 'ack.cmd'), { force: true });
  writeFileSync(
    path.join(binDir, 'ack.ps1'),
    `& node "${ackEntry}" @args\r\nexit $LASTEXITCODE\r\n`,
  );
  return binDir;
}

export function resolveProjectName(ackHome, repoRoot) {
  const registryPath = path.join(ackHome, 'projects.json');
  if (!existsSync(registryPath)) {
    throw new Error(`ACK registry not found: ${registryPath}; run ack init with this ACK_HOME first`);
  }
  const normalize = (value) => path.resolve(value).toLowerCase();
  const registry = JSON.parse(readFileSync(registryPath, 'utf8'));
  const entry = (registry.projects ?? []).find(
    (project) => normalize(project.rootPath) === normalize(repoRoot),
  );
  if (entry === undefined) {
    throw new Error(`no ACK project registered for ${repoRoot} in ${registryPath}`);
  }
  return entry.project;
}
