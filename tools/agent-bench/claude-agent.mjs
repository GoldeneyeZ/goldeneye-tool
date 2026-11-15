// A second agent for the task benchmark: Claude Code in print mode. Its event
// stream is translated to the events the harness already reads from Codex
// (`item.completed` with `command_execution`, `file_change` or
// `agent_message`, then `turn.completed` with the token usage), so telemetry,
// traces and reports are the same for both agents.

export const CLAUDE_TOOLS = ["Bash", "Read", "Edit", "Write", "Glob", "Grep"];
export const CLAUDE_DEFAULT_MODEL = "claude-opus-5-5";

const FILE_TOOLS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);
const FINAL_ANSWER_TOOL = "StructuredOutput";

/**
 * Arguments for `claude`: one turn, events on stdout, the prompt on stdin.
 * No user or project customization is loaded (the Codex lane runs with
 * `--ignore-user-config --ignore-rules`), no MCP server, no sub-agent, and
 * only the built-in shell and file tools, each allowed without a prompt.
 */
export function claudeArgs({ model, effort, schema }) {
  const args = [
    "-p",
    "--output-format",
    "stream-json",
    "--verbose",
    "--safe-mode",
    "--strict-mcp-config",
    "--disable-slash-commands",
    "--no-session-persistence",
    "--tools",
    CLAUDE_TOOLS.join(","),
    "--allowedTools",
    CLAUDE_TOOLS.join(","),
    "--permission-mode",
    "acceptEdits",
    "--model",
    model ?? CLAUDE_DEFAULT_MODEL,
  ];
  if (effort) args.push("--effort", effort);
  if (schema) {
    // The CLI validates the schema and does not know the meta-schema's URL.
    const { $schema: _dialect, ...plain } = schema;
    args.push("--json-schema", JSON.stringify(plain));
  }
  return args;
}

/**
 * The environment for the child: without the variables a Claude session that
 * launched the benchmark leaves behind (they make the child expect its host
 * to refresh the login, and point it at the host's session).
 */
export function claudeEnvironment(environment) {
  const cleaned = {};
  for (const [key, value] of Object.entries(environment)) {
    if (/^(?:CLAUDECODE$|CLAUDE_CODE_|CLAUDE_AGENT_SDK_|CLAUDE_PID$|CLAUDE_EFFORT$|CLAUDE_PREVIEW_)/.test(key)) continue;
    if (key === "ANTHROPIC_BASE_URL" && environment.ANTHROPIC_API_KEY === undefined) continue;
    cleaned[key] = value;
  }
  cleaned.CLAUDE_CODE_DISABLE_AUTO_MEMORY = "1";
  return cleaned;
}

function resultText(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((part) => (typeof part === "string" ? part : part?.type === "text" ? String(part.text ?? "") : ""))
    .join("");
}

/** A discovery tool written as one line, the way a shell command is recorded. */
function renderTool(name, input = {}) {
  if (name === "Read") {
    const from = Number(input.offset);
    const count = Number(input.limit);
    const range = Number.isFinite(from) ? `:${from}${Number.isFinite(count) ? `-${from + count - 1}` : ""}` : "";
    return `Read ${input.file_path ?? ""}${range}`;
  }
  if (name === "Grep") {
    const options = [
      input.glob ? `--glob ${input.glob}` : "",
      input.type ? `--type ${input.type}` : "",
      input.output_mode ? `--${input.output_mode}` : "",
      ...["-A", "-B", "-C"].map((flag) => (input[flag] === undefined ? "" : `${flag} ${input[flag]}`)),
      input.context === undefined ? "" : `-C ${input.context}`,
      input["-i"] ? "-i" : "",
      input.multiline ? "--multiline" : "",
    ].filter(Boolean);
    return ["Grep", JSON.stringify(String(input.pattern ?? "")), input.path ?? ".", ...options].join(" ");
  }
  if (name === "Glob") return ["Glob", input.pattern ?? "", input.path ?? ""].join(" ").trim();
  return `${name} ${JSON.stringify(input)}`;
}

/** `{"summary": ..., "tests": [...]}` as text, whatever form the final answer took. */
export function finalResponseText(answer) {
  if (answer !== null && typeof answer === "object") return JSON.stringify(answer);
  const text = String(answer ?? "").trim();
  const fenced = /^```(?:json)?\s*([\s\S]*?)\s*```$/.exec(text)?.[1] ?? text;
  try {
    const parsed = JSON.parse(fenced);
    if (parsed !== null && typeof parsed === "object" && typeof parsed.summary === "string") {
      return JSON.stringify(parsed);
    }
  } catch {
    // Prose: the answer is the summary.
  }
  return JSON.stringify({ summary: text, tests: [] });
}

/**
 * Translates the lines of `claude -p --output-format stream-json` into
 * Codex-shaped events. `push(line)` returns the events that line completes.
 *
 * - Bash: `command_execution` with the command, its output and exit code.
 * - Read, Grep, Glob: `command_execution` too (one tool call each), the
 *   command being the tool and its arguments; `tool` names the tool.
 * - Edit, Write: `file_change`, like a Codex patch (not counted as a tool call).
 * - The final answer (the StructuredOutput call, or the result text):
 *   `agent_message`, then `turn.completed` with the usage of the whole run.
 *
 * Input tokens are counted as Codex counts them: everything sent, cached or
 * not (`input + cache_creation + cache_read`); `cached_input_tokens` is what
 * was read from the cache.
 */
export function createClaudeTranslator() {
  const pending = new Map();
  const messageUsage = new Map();
  let structured;
  let finalText = null;
  let sequence = 0;
  const state = { error: null, completed: false, turns: 0 };

  const item = (fields) => ({ type: "item.completed", item: { id: `item_${sequence++}`, ...fields } });

  function toolResult(block) {
    const call = pending.get(block.tool_use_id);
    if (call === undefined) return [];
    pending.delete(block.tool_use_id);
    const output = resultText(block.content);
    const failed = block.is_error === true;
    const status = failed ? "failed" : "completed";
    if (call.name === FINAL_ANSWER_TOOL) return [];
    if (FILE_TOOLS.has(call.name)) {
      const path = call.input?.file_path ?? call.input?.notebook_path ?? "";
      return [item({ type: "file_change", changes: [{ path, kind: call.name === "Write" ? "add" : "update" }], status })];
    }
    if (call.name === "Bash" || call.name === "PowerShell") {
      const code = Number(/^Exit code (\d+)/m.exec(output)?.[1] ?? (failed ? 1 : 0));
      return [
        item({
          type: "command_execution",
          command: String(call.input?.command ?? ""),
          aggregated_output: output,
          exit_code: code,
          status,
          tool: call.name,
        }),
      ];
    }
    return [
      item({
        type: "command_execution",
        command: renderTool(call.name, call.input),
        aggregated_output: output,
        exit_code: failed ? 1 : 0,
        status,
        tool: call.name,
      }),
    ];
  }

  function sumUsage() {
    const total = { input_tokens: 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 0 };
    for (const usage of messageUsage.values()) {
      for (const key of Object.keys(total)) total[key] += Number(usage[key] ?? 0);
    }
    return total;
  }

  function push(line) {
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      return [];
    }
    const blocks = Array.isArray(event?.message?.content) ? event.message.content : [];
    if (event.type === "assistant") {
      const message = event.message ?? {};
      if (event.error) state.error = `${event.error}: ${resultText(blocks)}`.slice(0, 500);
      if (message.id && message.usage && message.model !== "<synthetic>") {
        if (!messageUsage.has(message.id)) state.turns += 1;
        messageUsage.set(message.id, message.usage);
      }
      const events = [];
      for (const block of blocks) {
        if (block.type === "text" && String(block.text ?? "").trim()) {
          finalText = block.text;
          events.push(item({ type: "agent_message", text: block.text }));
        } else if (block.type === "tool_use") {
          pending.set(block.id, { name: block.name, input: block.input });
          if (block.name === FINAL_ANSWER_TOOL) structured = block.input;
        }
      }
      return events;
    }
    if (event.type === "user") {
      return blocks.filter((block) => block?.type === "tool_result").flatMap(toolResult);
    }
    if (event.type !== "result") return [];

    state.completed = true;
    if (event.is_error) state.error = String(event.result ?? event.subtype ?? "claude reported an error").slice(0, 500);
    const answer = structured ?? event.structured_output ?? event.result ?? finalText ?? "";
    const text = finalResponseText(answer);
    const reported = event.usage ?? {};
    const summed = sumUsage();
    const total = (usage) =>
      Number(usage.input_tokens ?? 0) +
      Number(usage.cache_creation_input_tokens ?? 0) +
      Number(usage.cache_read_input_tokens ?? 0);
    // The result carries the usage of the whole run; the sum over the
    // messages stands in when it is missing.
    const usage = total(reported) > 0 ? reported : summed;
    return [
      ...(state.error === null ? [] : [{ type: "error", message: state.error }]),
      item({ type: "agent_message", text }),
      {
        type: "turn.completed",
        usage: {
          input_tokens: total(usage),
          cached_input_tokens: Number(usage.cache_read_input_tokens ?? 0),
          output_tokens: Number(usage.output_tokens ?? 0),
          reasoning_output_tokens: Number(usage.output_tokens_details?.thinking_tokens ?? 0),
        },
        claude: {
          model_turns: state.turns,
          num_turns: event.num_turns ?? null,
          total_cost_usd: event.total_cost_usd ?? null,
          cache_creation_input_tokens: Number(usage.cache_creation_input_tokens ?? 0),
          summed_message_tokens: summed,
          models: Object.keys(event.modelUsage ?? {}),
        },
      },
    ];
  }

  return { push, state };
}
