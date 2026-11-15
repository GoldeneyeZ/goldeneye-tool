import assert from "node:assert/strict";
import test from "node:test";
import {
  CLAUDE_DEFAULT_MODEL,
  claudeArgs,
  claudeEnvironment,
  createClaudeTranslator,
  finalResponseText,
} from "./claude-agent.mjs";
import { parseCodexJsonl } from "./core.mjs";
import { agentMessageText, isAgentProtocolCompletionLine } from "./timing.mjs";

const assistant = (id, content, usage = { input_tokens: 10, cache_read_input_tokens: 100, output_tokens: 5 }) =>
  JSON.stringify({ type: "assistant", message: { id, model: "claude-opus-5-5", usage, content } });
const toolUse = (id, name, input) => ({ type: "tool_use", id, name, input });
const toolResult = (id, content, isError = false) =>
  JSON.stringify({
    type: "user",
    message: { content: [{ type: "tool_result", tool_use_id: id, content, is_error: isError }] },
  });

function translate(lines) {
  const translator = createClaudeTranslator();
  const events = lines.flatMap((line) => translator.push(line));
  return { events, state: translator.state, jsonl: events.map((event) => JSON.stringify(event)).join("\n") };
}

test("claude arguments run one isolated turn with the shell and file tools only", () => {
  const args = claudeArgs({
    model: undefined,
    effort: "high",
    schema: { $schema: "https://json-schema.org/draft/2020-12/schema", type: "object" },
  });
  assert.equal(args[args.indexOf("--model") + 1], CLAUDE_DEFAULT_MODEL);
  assert.equal(args[args.indexOf("--tools") + 1], "Bash,Read,Edit,Write,Glob,Grep");
  assert.equal(args[args.indexOf("--effort") + 1], "high");
  assert.equal(args[args.indexOf("--json-schema") + 1], '{"type":"object"}');
  for (const flag of ["-p", "--safe-mode", "--strict-mcp-config", "--no-session-persistence"]) {
    assert.ok(args.includes(flag), flag);
  }
  assert.ok(!args.some((arg) => /bypass|dangerously/i.test(arg)));
});

test("the child environment drops what a host Claude session leaves behind", () => {
  const environment = claudeEnvironment({
    PATH: "p",
    GCAL_HOME: "h",
    CLAUDECODE: "1",
    CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH: "1",
    CLAUDE_AGENT_SDK_VERSION: "0.3",
    ANTHROPIC_BASE_URL: "https://host.example",
  });
  assert.deepEqual(environment, { PATH: "p", GCAL_HOME: "h", CLAUDE_CODE_DISABLE_AUTO_MEMORY: "1" });
  const keyed = claudeEnvironment({ ANTHROPIC_API_KEY: "k", ANTHROPIC_BASE_URL: "https://proxy.example" });
  assert.equal(keyed.ANTHROPIC_BASE_URL, "https://proxy.example");
});

test("tool calls become the events the harness reads from Codex", () => {
  const { events, jsonl } = translate([
    JSON.stringify({ type: "system", subtype: "init" }),
    assistant("m1", [
      { type: "text", text: "Looking." },
      toolUse("t1", "Bash", { command: "gcal callers Billing.charge; git status --short" }),
      toolUse("t2", "Read", { file_path: "src/a.py", offset: 10, limit: 20 }),
    ]),
    toolResult("t1", "# callers of charge: 2\n M src/a.py"),
    toolResult("t2", [{ type: "text", text: "10\tdef f():\n" }]),
    assistant("m2", [
      toolUse("t3", "Grep", { pattern: "charge\\(", path: "src", glob: "*.py", output_mode: "content", "-C": 2 }),
      toolUse("t4", "Edit", { file_path: "src/a.py", old_string: "a", new_string: "b" }),
      toolUse("t5", "Bash", { command: "false" }),
    ]),
    toolResult("t3", "src/a.py:3:charge("),
    toolResult("t4", "The file src/a.py has been updated."),
    toolResult("t5", "Exit code 3\nboom", true),
  ]);

  const items = events.map((event) => event.item);
  assert.deepEqual(
    items.map((item) => item.type),
    ["agent_message", "command_execution", "command_execution", "command_execution", "file_change", "command_execution"],
  );
  assert.equal(items[1].command, "gcal callers Billing.charge; git status --short");
  assert.equal(items[1].exit_code, 0);
  assert.equal(items[2].command, "Read src/a.py:10-29");
  assert.equal(items[2].aggregated_output, "10\tdef f():\n");
  assert.equal(items[3].command, 'Grep "charge\\\\(" src --glob *.py --content -C 2');
  assert.deepEqual(items[4].changes, [{ path: "src/a.py", kind: "update" }]);
  assert.equal(items[5].exit_code, 3);

  const telemetry = parseCodexJsonl(jsonl);
  assert.equal(telemetry.tool_calls, 4);
  assert.equal(telemetry.command_calls, 4);
  assert.equal(telemetry.gcal_calls, 1);
  assert.equal(telemetry.waste.patches, 1);
  assert.equal(telemetry.waste.failed_commands, 1);
});

test("the result closes the turn with the final answer and the usage of the whole run", () => {
  const { events, state } = translate([
    assistant("m1", [toolUse("t1", "Bash", { command: "ls" })]),
    assistant("m1", [toolUse("t1", "Bash", { command: "ls" })]),
    toolResult("t1", "a.txt"),
    assistant("m2", [toolUse("t2", "StructuredOutput", { summary: "done", tests: [] })]),
    toolResult("t2", "Structured output provided successfully"),
    JSON.stringify({
      type: "result",
      subtype: "success",
      is_error: false,
      num_turns: 3,
      total_cost_usd: 0.12,
      result: "",
      usage: {
        input_tokens: 30,
        cache_creation_input_tokens: 2_000,
        cache_read_input_tokens: 50_000,
        output_tokens: 400,
        output_tokens_details: { thinking_tokens: 150 },
      },
      modelUsage: { "claude-opus-5-5": {} },
    }),
  ]);

  const lines = events.map((event) => JSON.stringify(event));
  assert.equal(agentMessageText(lines.at(-2)), '{"summary":"done","tests":[]}');
  assert.ok(isAgentProtocolCompletionLine(lines.at(-1)));
  assert.equal(state.error, null);
  assert.equal(state.turns, 2);

  const telemetry = parseCodexJsonl(lines.join("\n"));
  assert.equal(telemetry.input_tokens, 52_030);
  assert.equal(telemetry.cached_input_tokens, 50_000);
  assert.equal(telemetry.output_tokens, 400);
  assert.equal(telemetry.reasoning_output_tokens, 150);
  assert.equal(telemetry.tool_calls, 1);
  assert.equal(events.at(-1).claude.cache_creation_input_tokens, 2_000);
});

test("a failed login is reported as the run's error, and prose is wrapped as the summary", () => {
  const { events, state } = translate([
    JSON.stringify({
      type: "assistant",
      error: "authentication_failed",
      message: { id: "x", model: "<synthetic>", usage: {}, content: [{ type: "text", text: "Failed to authenticate" }] },
    }),
    JSON.stringify({ type: "result", is_error: true, result: "Failed to authenticate", usage: {} }),
  ]);
  assert.equal(state.error, "Failed to authenticate");
  assert.equal(events.find((event) => event.type === "error").message, "Failed to authenticate");
  assert.equal(events.at(-1).usage.input_tokens, 0);

  assert.equal(finalResponseText("All good."), '{"summary":"All good.","tests":[]}');
  assert.equal(finalResponseText('```json\n{"summary":"s","tests":["t"]}\n```'), '{"summary":"s","tests":["t"]}');
  assert.equal(finalResponseText({ summary: "s", tests: [] }), '{"summary":"s","tests":[]}');
});
