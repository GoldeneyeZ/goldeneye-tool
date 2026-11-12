#!/usr/bin/env node
// Grades a written answer (exploration and documentation tasks) against a
// checklist of facts instead of running tests.
//
//   node fact-checklist.mjs --checklist <file.json> --run-dir {runDir} --worktree {worktree}
//
// Checklist:
//   {
//     "source": "final-message" | "<path inside the worktree>",
//     "facts": [{ "id": "...", "all": ["<regex>", ...], "any": ["<regex>", ...] }],
//     "forbidden": [{ "id": "...", "any": ["<regex>", ...] }],
//     "min_facts": <number, default: every fact>
//   }
// A fact holds when every `all` pattern and at least one `any` pattern match.
// Patterns are case-insensitive regular expressions.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const matches = (pattern, text) => new RegExp(pattern, "is").test(text);

export function evaluateChecklist(checklist, text) {
  const facts = checklist.facts ?? [];
  const holds = (fact) =>
    (fact.all ?? []).every((pattern) => matches(pattern, text)) &&
    ((fact.any ?? []).length === 0 || fact.any.some((pattern) => matches(pattern, text)));
  const missing = facts.filter((fact) => !holds(fact)).map((fact) => fact.id);
  const forbidden = (checklist.forbidden ?? [])
    .filter((entry) => (entry.any ?? []).some((pattern) => matches(pattern, text)))
    .map((entry) => entry.id);
  const found = facts.length - missing.length;
  const required = checklist.min_facts ?? facts.length;
  return {
    passed: text.trim().length > 0 && found >= required && forbidden.length === 0,
    facts: facts.length,
    found,
    required,
    missing,
    forbidden,
  };
}

// The agent's final message is JSON with a `summary` field; anything else is read as text.
export function answerText(raw) {
  try {
    const parsed = JSON.parse(raw);
    if (typeof parsed?.summary === "string") return parsed.summary;
  } catch {
    // Not JSON: the file itself is the answer.
  }
  return raw;
}

function option(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || index + 1 >= process.argv.length) {
    console.error(`fact-checklist: missing ${name}`);
    process.exit(2);
  }
  return process.argv[index + 1];
}

function main() {
  const checklist = JSON.parse(readFileSync(resolve(option("--checklist")), "utf8"));
  const source = checklist.source ?? "final-message";
  const path =
    source === "final-message"
      ? join(resolve(option("--run-dir")), "final-message.txt")
      : join(resolve(option("--worktree")), source);

  let text = "";
  try {
    const raw = readFileSync(path, "utf8");
    text = source === "final-message" ? answerText(raw) : raw;
  } catch (error) {
    console.error(`fact-checklist: cannot read ${path}: ${error.message}`);
  }

  const result = evaluateChecklist(checklist, text);
  console.log(JSON.stringify({ source, ...result }, null, 2));
  process.exit(result.passed ? 0 : 1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
