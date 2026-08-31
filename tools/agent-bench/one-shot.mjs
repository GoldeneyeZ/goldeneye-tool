import { randomBytes } from "node:crypto";
import { join, resolve } from "node:path";
import { sanitizeId } from "./core.mjs";

const INCOMPATIBLE_WORKFLOW_FLAGS = new Set([
  "--prepare-snapshot",
  "--verify-only",
  "--smoke",
  "--calibration",
  "--audit-report",
]);

export function validateOneShotOptions({
  enabled,
  tasks = [],
  engines = [],
  cacheModes = [],
  repetitions = 1,
  activeWorkflowFlags = [],
  attemptId = null,
  skipAgentVerification = false,
}) {
  if (attemptId === true) throw new Error("--attempt-id requires a value");
  if (!enabled) {
    if (attemptId) throw new Error("--attempt-id requires --one-shot");
    return { enabled: false, skipAgentVerification: Boolean(skipAgentVerification) };
  }

  for (const flag of activeWorkflowFlags) {
    if (INCOMPATIBLE_WORKFLOW_FLAGS.has(flag)) {
      throw new Error(`--one-shot is incompatible with ${flag}`);
    }
  }
  if (tasks.length !== 1) throw new Error("--one-shot requires exactly one task");
  if (engines.length !== 1) throw new Error("--one-shot requires exactly one engine");
  if (cacheModes.length !== 1) throw new Error("--one-shot requires exactly one cache mode");
  if (repetitions !== 1) throw new Error("--one-shot requires exactly one repetition");

  return { enabled: true, skipAgentVerification: true };
}

export function resolveOneShotAttemptId(
  value,
  {
    now = Date.now,
    random = () => randomBytes(4).toString("hex"),
  } = {},
) {
  if (value) return sanitizeId(value);
  const timestamp = new Date(now()).toISOString().replace(/[-:.]/g, "").toLowerCase();
  return sanitizeId(`${timestamp}-${random()}`);
}

export function resolveOneShotOutput({ workspace, taskId, attemptId, explicitOutput }) {
  if (explicitOutput) return resolve(explicitOutput);
  return join(
    resolve(workspace),
    "target",
    "agent-bench",
    sanitizeId(taskId),
    "one-shot",
    sanitizeId(attemptId),
    "report.json",
  );
}

export function agentVerificationPolicy() {
  return [
    "One-shot execution policy (final override):",
    "- Do not run build, compile, test, lint, check, verification, or validation commands.",
    "- Make the requested source edits, inspect the resulting diff/status if needed, then finish.",
    "- GCAL discovery calls are not limited; prefer one gcal workflow --js/--file invocation when later discovery depends on earlier results.",
    "- The benchmark harness runs the held-out grader after you exit.",
  ].join("\n");
}

export function applyAgentVerificationPolicy(prompt, enabled) {
  if (!enabled) return prompt;
  return `${String(prompt).trimEnd()}\n\n${agentVerificationPolicy()}`;
}

export function isAgentVerificationCommand(command) {
  const rendered = renderCommand(command)
    .replace(/\\/g, "/")
    .replace(/^[\s"']+|[\s"']+$/g, "")
    .toLowerCase();
  if (!rendered) return false;

  const executable = String.raw`(?:^|[;&|]\s*|\s)(?:\.\/)?`;
  const goal = String.raw`(?:build|check|compile\w*|test\w*|verify|package|install|assemble|lint\w*|clippy)`;
  return (
    new RegExp(`${executable}(?:mvn|mvnw)(?:\\.cmd|\\.bat)?\\b[^;&|]*(?:^|\\s)${goal}\\b`, "i").test(rendered) ||
    new RegExp(`${executable}(?:gradle|gradlew)(?:\\.cmd|\\.bat)?\\b[^;&|]*(?:^|\\s)${goal}\\b`, "i").test(rendered) ||
    new RegExp(`${executable}cargo\\s+(?:build|check|test|clippy)\\b`, "i").test(rendered) ||
    new RegExp(`${executable}(?:npm|pnpm|yarn)(?:\\.cmd)?\\s+(?:(?:run|run-script)\\s+)?(?:build|check|compile\\w*|test\\w*|lint\\w*)\\b`, "i").test(rendered) ||
    new RegExp(`${executable}(?:npx\\s+)?(?:tsc|eslint|javac|rustc)(?:\\.exe|\\.cmd)?\\b`, "i").test(rendered) ||
    new RegExp(`${executable}(?:pytest|jest|vitest|mocha|ava|phpunit)(?:\\.exe|\\.cmd)?(?:\\s|$)`, "i").test(rendered) ||
    new RegExp(`${executable}(?:go\\s+test|node\\s+--test)\\b`, "i").test(rendered) ||
    new RegExp(`${executable}(?:dotnet\\s+(?:build|test)|cmake\\s+--build|make(?:\\.exe)?(?:\\s|$))`, "i").test(rendered)
  );
}

export function analyzeAgentVerificationCalls(commandCalls = []) {
  const agentVerificationCalls = commandCalls
    .map((call) => ({
      command: renderCommand(call?.command ?? call?.cmd),
      exit_code: normalizeExitCode(call?.exit_code ?? call?.exitCode),
    }))
    .filter((call) => isAgentVerificationCommand(call.command));
  return {
    agent_verification_calls: agentVerificationCalls,
    one_shot_compliant: agentVerificationCalls.length === 0,
  };
}

function renderCommand(command) {
  return Array.isArray(command) ? command.join(" ") : String(command ?? "");
}

function normalizeExitCode(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}
