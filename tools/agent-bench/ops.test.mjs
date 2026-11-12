import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_COST_MODEL,
  chainCost,
  compileEvidence,
  estimateTokens,
  expandPlaceholders,
  missingEvidence,
  renderMarkdown,
  runChain,
  summarize,
} from './ops.mjs';

function scriptedExecutor(outputs) {
  const calls = [];
  const execute = (command) => {
    calls.push(command);
    const scripted = outputs[command];
    if (scripted === undefined) throw new Error(`unexpected command: ${command}`);
    return { output: scripted.output, exitCode: scripted.exitCode ?? 0, ms: scripted.ms ?? 10 };
  };
  return { execute, calls };
}

test('an extra call costs more than a few kilobytes of output', () => {
  const oneLargeCall = chainCost([estimateTokens('x'.repeat(8_000))]);
  const twoSmallCalls = chainCost([estimateTokens('x'.repeat(400)), estimateTokens('x'.repeat(400))]);

  assert.equal(chainCost([]), 0);
  assert.equal(chainCost([0]), DEFAULT_COST_MODEL.baseContextTokens * DEFAULT_COST_MODEL.cachedPriceRatio);
  assert.ok(twoSmallCalls < oneLargeCall, 'small outputs still win when the gap is 7 KB');
  assert.ok(
    chainCost([estimateTokens('x'.repeat(4_000))]) < chainCost([100, 100]),
    'one 4 KB call beats two 400 B calls',
  );
});

test('evidence accepts literals and regular expressions', () => {
  const compiled = compileEvidence(['needle', { regex: 'LIMIT\\s*=\\s*256' }]);

  assert.deepEqual(missingEvidence(compiled, 'a needle, LIMIT = 256'), []);
  assert.deepEqual(missingEvidence(compiled, 'only a needle'), ['/LIMIT\\s*=\\s*256/']);
  assert.throws(() => compileEvidence([{ pattern: 'x' }]), /invalid evidence entry/);
});

test('placeholders expand and unknown ones are rejected', () => {
  assert.equal(expandPlaceholders("gcal get '${P}.a.B'", { P: 'proj' }), "gcal get 'proj.a.B'");
  assert.throws(() => expandPlaceholders('${NOPE}', {}), /unknown placeholder/);
});

test('a chain stops at the first step that completes the evidence', () => {
  const { execute, calls } = scriptedExecutor({
    first: { output: 'alpha' },
    second: { output: 'beta' },
    third: { output: 'never needed' },
  });

  const run = runChain({ steps: ['first', 'second', 'third'], evidence: ['alpha', 'beta'], execute });

  assert.deepEqual(calls, ['first', 'second']);
  assert.equal(run.ok, true);
  assert.equal(run.calls, 2);
  assert.equal(run.failed_calls, 0);
  assert.equal(run.cost, chainCost(run.steps.map((step) => step.tokens)));
});

test('recovery steps run only once an earlier command failed', () => {
  const steps = ['natural', { cmd: 'recover', when: 'after-failure' }, 'follow-up'];
  const failing = scriptedExecutor({
    natural: { output: 'not found', exitCode: 1 },
    recover: { output: 'alpha' },
    'follow-up': { output: 'beta' },
  });
  const succeeding = scriptedExecutor({
    natural: { output: 'alpha' },
    'follow-up': { output: 'beta' },
  });

  const recovered = runChain({ steps, evidence: ['alpha', 'beta'], execute: failing.execute });
  const direct = runChain({ steps, evidence: ['alpha', 'beta'], execute: succeeding.execute });

  assert.deepEqual(failing.calls, ['natural', 'recover', 'follow-up']);
  assert.equal(recovered.failed_calls, 1);
  assert.deepEqual(succeeding.calls, ['natural', 'follow-up']);
  assert.equal(direct.calls, 2);
});

test('an exhausted chain reports the evidence it never found', () => {
  const { execute } = scriptedExecutor({ only: { output: 'alpha' } });

  const run = runChain({ steps: ['only'], evidence: ['alpha', 'omega'], execute });

  assert.equal(run.ok, false);
  assert.deepEqual(run.missing_evidence, ['omega']);
});

test('summary and markdown aggregate per lane and category', () => {
  const lane = (ok, calls, tokens) => ({
    ok,
    calls,
    failed_calls: ok ? 0 : 1,
    bytes: tokens * 4,
    tokens,
    ms: calls * 100,
    cost: chainCost(Array(calls).fill(tokens / calls)),
    steps: [],
  });
  const scenarios = [
    { id: 'read', category: 'read-symbol', lanes: { gcal: lane(true, 1, 100), vanilla: lane(true, 2, 400) } },
    { id: 'grep', category: 'text', lanes: { gcal: lane(false, 2, 50), vanilla: lane(true, 1, 80) } },
  ];
  const lanes = ['gcal', 'vanilla'];
  const summary = summarize(scenarios, lanes);

  assert.deepEqual(
    { ok: summary.totals.gcal.ok, calls: summary.totals.gcal.calls, failed: summary.totals.gcal.failed_calls },
    { ok: 1, calls: 3, failed: 1 },
  );
  assert.equal(summary.by_category.text.vanilla.ok, 1);

  const markdown = renderMarkdown({
    corpus: 'fixture',
    label: 'unit',
    lanes,
    cost_model: DEFAULT_COST_MODEL,
    scenarios,
    summary,
  });
  assert.match(markdown, /\| read \| read-symbol \| ok 1c 100t .* \| gcal x\d/);
  assert.match(markdown, /\| grep \| text \| FAIL 2c 50t .* \| vanilla \|/);
  assert.match(markdown, /\| gcal \| 1\/2 \| 3 \| 1 \|/);
});
