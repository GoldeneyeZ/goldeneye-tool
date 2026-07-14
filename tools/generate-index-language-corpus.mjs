#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const root = path.resolve(import.meta.dirname, "..");
const reproDir = path.join(
  root,
  ".upstream",
  "codebase-memory-mcp",
  "tests",
  "repro",
);
const generatedRegistry = fs.readFileSync(
  path.join(root, "crates", "goldeneye-full-grammars", "src", "generated.rs"),
  "utf8",
);

function balancedBlock(source, openIndex, open = "{", close = "}") {
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  for (let index = openIndex; index < source.length; index += 1) {
    const character = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (character === "\n") lineComment = false;
      continue;
    }
    if (blockComment) {
      if (character === "*" && next === "/") {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (quote !== null) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === "/" && next === "/") {
      lineComment = true;
      index += 1;
      continue;
    }
    if (character === "/" && next === "*") {
      blockComment = true;
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
    } else if (character === open) {
      depth += 1;
    } else if (character === close && --depth === 0) {
      return source.slice(openIndex + 1, index);
    }
  }
  throw new Error("unterminated TEST block");
}

function splitArguments(source) {
  const argumentsList = [];
  let start = 0;
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (quote !== null) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'") quote = character;
    else if ("([{".includes(character)) depth += 1;
    else if (")] }".replace(" ", "").includes(character)) depth -= 1;
    else if (character === "," && depth === 0) {
      argumentsList.push(source.slice(start, index).trim());
      start = index + 1;
    }
  }
  argumentsList.push(source.slice(start).trim());
  return argumentsList;
}

function decodeStringArgument(argument) {
  if (argument.trim() === "NULL") return null;
  const literals = [...argument.matchAll(/"((?:\\.|[^"\\])*)"/gs)];
  if (literals.length === 0) return null;
  return literals.map((match) => decodeCString(match[1])).join("");
}

function decodeCString(value) {
  let output = "";
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (character !== "\\") {
      output += character;
      continue;
    }
    const escaped = value[++index];
    const simple = {
      a: "\u0007",
      b: "\b",
      f: "\f",
      n: "\n",
      r: "\r",
      t: "\t",
      v: "\u000b",
      "\\": "\\",
      '"': '"',
      "'": "'",
      "?": "?",
    };
    if (Object.hasOwn(simple, escaped)) {
      output += simple[escaped];
    } else if (escaped === "x") {
      const match = value.slice(index + 1).match(/^[0-9a-fA-F]+/);
      if (!match) throw new Error(`invalid hex escape in ${value}`);
      output += String.fromCodePoint(Number.parseInt(match[0], 16));
      index += match[0].length;
    } else if (/[0-7]/.test(escaped)) {
      const match = value.slice(index).match(/^[0-7]{1,3}/)[0];
      output += String.fromCodePoint(Number.parseInt(match, 8));
      index += match.length - 1;
    } else if (escaped === "\n") {
      // C line continuation.
    } else {
      throw new Error(`unsupported C escape \\${escaped}`);
    }
  }
  return output;
}

function fixtureFromTest(testName, body, sourceFile, signatures) {
  const sourceStart = body.indexOf("static const char src[]");
  if (sourceStart === -1) return null;
  const initializerStart = body.indexOf("=", sourceStart);
  const initializerMatch = body
    .slice(initializerStart + 1)
    .match(/^\s*((?:"(?:\\.|[^"\\])*"\s*)+);/s);
  if (!initializerMatch) throw new Error(`${testName}: unterminated src initializer`);
  const initializer = initializerMatch[1];
  const source = [...initializer.matchAll(/"((?:\\.|[^"\\])*)"/gs)]
    .map((match) => decodeCString(match[1]))
    .join("");
  const languageMatch = body.match(/CBM_LANG_([A-Z0-9_]+)/);
  if (!languageMatch) throw new Error(`${testName}: missing CBM_LANG argument`);
  const language = languageMatch[1].toLowerCase();
  const pathMatch = body.match(
    new RegExp(`CBM_LANG_${languageMatch[1]}\\s*,\\s*"([^"]+)"`),
  );
  if (!pathMatch) throw new Error(`${testName}: missing fixture path after CBM_LANG`);
  const invocationPrefix = body.slice(0, languageMatch.index);
  const invocationMatches = [...invocationPrefix.matchAll(/([a-z_]+_battery)\s*\(/g)];
  const invocation = invocationMatches.at(-1);
  if (!invocation) throw new Error(`${testName}: missing battery invocation`);
  const helper = invocation[1];
  const openIndex = invocation.index + invocation[0].lastIndexOf("(");
  const argumentValues = splitArguments(balancedBlock(body, openIndex, "(", ")"));
  const parameters = signatures.get(helper);
  if (!parameters || parameters.length !== argumentValues.length) {
    throw new Error(
      `${testName}: ${helper} signature/argument mismatch (${parameters?.length}/${argumentValues.length})`,
    );
  }
  const values = new Map(
    parameters.map((parameter, index) => [parameter, decodeStringArgument(argumentValues[index])]),
  );
  const expectedLabels = parameters
    .filter((parameter) => parameter.startsWith("expect_label"))
    .map((parameter) => values.get(parameter))
    .filter((value) => value !== null);
  return {
    language,
    path: pathMatch[1],
    source,
    expectedLabels,
    callee: values.get("callee") ?? null,
    sourceFile,
    testName,
  };
}

let fixtures = [];
for (const filename of fs
  .readdirSync(reproDir)
  .filter((value) => /^repro_grammar_.*\.c$/.test(value))
  .sort()) {
  const source = fs.readFileSync(path.join(reproDir, filename), "utf8");
  const signatures = new Map();
  for (const signature of source.matchAll(/static int ([a-z_]+_battery)\((.*?)\)\s*\{/gs)) {
    const parameters = [...signature[2].matchAll(/(?:CBMLanguage|const char\s*\*)\s*(\w+)/g)]
      .map((match) => match[1]);
    signatures.set(signature[1], parameters);
  }
  for (const match of source.matchAll(/TEST\((repro_grammar_[^)]+)\)\s*\{/g)) {
    const openIndex = match.index + match[0].lastIndexOf("{");
    const fixture = fixtureFromTest(
      match[1],
      balancedBlock(source, openIndex),
      filename,
      signatures,
    );
    if (fixture) fixtures.push(fixture);
  }
}

const availableIds = [...generatedRegistry.matchAll(/GeneratedLanguage \{ id: "([^"]+)", availability: GeneratedAvailability::Available/g)]
  .map((match) => match[1])
  .sort();
// The repro battery intentionally contains Nim as a documented unsupported RED
// row, while the callable pack contains Mojo. Translate that row using the Mojo
// fixture audited in upstream tests/test_grammar_regression.c.
fixtures = fixtures.filter((fixture) => availableIds.includes(fixture.language));
fixtures.push({
  language: "mojo",
  path: "a.mojo",
  source:
    "fn foo() -> Int:\n    return 1\n\nstruct A:\n    fn bar(self) -> Int:\n        return foo()\n",
  expectedLabels: ["Function", "Class"],
  callee: "foo",
  sourceFile: "test_grammar_regression.c",
  testName: "grammar_cases.mojo",
});
const fixtureIds = fixtures.map((fixture) => fixture.language).sort();
const missing = availableIds.filter((id) => !fixtureIds.includes(id));
const extra = fixtureIds.filter((id) => !availableIds.includes(id));
const duplicates = fixtureIds.filter((id, index) => id === fixtureIds[index - 1]);
if (missing.length || extra.length || duplicates.length || fixtures.length !== 159) {
  throw new Error(
    `corpus mismatch: fixtures=${fixtures.length}, missing=${missing.join(",")}, extra=${extra.join(",")}, duplicates=${duplicates.join(",")}`,
  );
}

const upstreamCommit = execFileSync(
  "git",
  ["-C", path.join(root, ".upstream", "codebase-memory-mcp"), "rev-parse", "HEAD"],
  { encoding: "utf8" },
).trim();
const importSignal = /(^|\n)\s*(?:#\s*(?:include|import)\b|@import\b|import\b|from\s+\S+\s+import\b|use\b|using\b|open\s+import\b|include\b|require\b|library\b|with\b)/m;
const relationExpectations = new Map([
  ["graphql", { inherits: [], implements: ["Node"] }],
  ["objc", { inherits: ["NSObject"], implements: [] }],
  ["smali", { inherits: ["Ljava/lang/Object"], implements: [] }],
  ["tsx", { inherits: ["React.Component"], implements: [] }],
]);
const lines = [
  "// @generated by tools/generate-index-language-corpus.mjs; do not edit manually.",
  `// Audited upstream corpus: DeusData/codebase-memory-mcp@${upstreamCommit}`,
  "",
  "#[derive(Debug, Clone, Copy)]",
  "pub(crate) struct LanguageFixture {",
  "    pub language: &'static str,",
  "    pub path: &'static str,",
  "    pub source: &'static str,",
  "    pub expected_labels: &'static [&'static str],",
  "    pub callee: Option<&'static str>,",
  "    pub expects_import: bool,",
  "    pub expected_inherits: &'static [&'static str],",
  "    pub expected_implements: &'static [&'static str],",
  "}",
  "",
  "pub(crate) static LANGUAGE_FIXTURES: &[LanguageFixture] = &[",
];
for (const fixture of fixtures.sort((left, right) => left.language.localeCompare(right.language))) {
  const relations = relationExpectations.get(fixture.language) ?? {
    inherits: [],
    implements: [],
  };
  lines.push("    LanguageFixture {");
  lines.push(`        language: ${JSON.stringify(fixture.language)},`);
  lines.push(`        path: ${JSON.stringify(fixture.path)},`);
  lines.push(`        source: ${JSON.stringify(fixture.source)},`);
  lines.push(
    `        expected_labels: &[${fixture.expectedLabels.map((label) => JSON.stringify(label)).join(", ")}],`,
  );
  lines.push(`        callee: ${fixture.callee === null ? "None" : `Some(${JSON.stringify(fixture.callee)})`},`);
  lines.push(`        expects_import: ${importSignal.test(fixture.source)},`);
  lines.push(
    `        expected_inherits: &[${relations.inherits.map((name) => JSON.stringify(name)).join(", ")}],`,
  );
  lines.push(
    `        expected_implements: &[${relations.implements.map((name) => JSON.stringify(name)).join(", ")}],`,
  );
  lines.push("    },");
}
lines.push("];");
lines.push("");
const rendered = lines.join("\n");
const outputArgument = process.argv[2];
if (outputArgument) {
  const outputPath = path.resolve(root, outputArgument);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, rendered);
} else {
  process.stdout.write(rendered);
}
