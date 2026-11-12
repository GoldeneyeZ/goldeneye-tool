#!/usr/bin/env node
// Structural grader for the task `goldeneye-rename-refactor-dirty`:
// rename `RevalidatedPath::as_path` (crate goldeneye-edit) to
// `checked_destination` on a dirty tree. Nothing is compiled.
//
//   node goldeneye-rename-refactor-dirty.mjs <worktree> [<source repo>]
//
// Exit 0 = pass. Any failure is printed and the exit code is 1.
//
// What is checked, over every `.rs` and `.md` file under `crates/` and `docs/`
// (directories named `target`, `.git` and `node_modules` are skipped):
//   1. the old identifier no longer appears as a whole word, except for calls
//      of the unrelated standard-library `PathBuf::as_path`, which are
//      whitelisted by file and receiver;
//   2. those unrelated items were not renamed (a blind search-and-replace
//      fails here);
//   3. the new identifier appears at the expected sites, including the ones
//      the overlay added (a new call, a new test file, a doc comment and a
//      Markdown paragraph);
//   4. the definition kept its signature, attribute and body apart from the
//      name, and is the only definition of the new name;
//   5. there is no `#[deprecated]` item and no forwarding alias;
//   6. the overlay's own work in progress is still there.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const OLD = "as_path";
const NEW = "checked_destination";

const worktree = resolve(process.argv[2] ?? ".");
const failures = [];

const SCAN_ROOTS = ["crates", "docs"];
const SKIP_DIRECTORIES = new Set(["target", ".git", "node_modules"]);

const DEFINITION_FILE = "crates/application/goldeneye-edit/src/path_auth.rs";
const OVERLAY_TEST_FILE = "crates/application/goldeneye-edit/tests/revalidated_path.rs";

// Whole-word uses of the old name that are NOT the renamed method. Each entry
// allows `<receiver>.as_path` in one file only: these are calls of
// `std::path::PathBuf::as_path` and must stay as they are.
const UNRELATED_OLD_USES = [
  {
    file: "docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery.md",
    receiver: "relative_path",
  },
  {
    file: "docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/tasks/GD-4/task.md",
    receiver: "relative_path",
  },
  { file: OVERLAY_TEST_FILE, receiver: "expected" },
];

// Minimum number of whole-word occurrences of the new name per file.
// "overlay" marks sites that only exist in the dirty working tree.
const EXPECTED_NEW_SITES = [
  // definition + intra-doc link added by the overlay (+ the `AsRef` body)
  { file: DEFINITION_FILE, min: 2, note: "definition and doc-comment link (overlay)" },
  { file: "crates/application/goldeneye-edit/src/durable/mutation.rs", min: 1, note: "caller" },
  { file: "crates/application/goldeneye-edit/src/durable/commit.rs", min: 1, note: "caller added by the overlay" },
  { file: "crates/application/goldeneye-edit/tests/path_auth.rs", min: 1, note: "test" },
  { file: OVERLAY_TEST_FILE, min: 2, note: "test file added by the overlay" },
  { file: "crates/application/goldeneye-services/src/edit/inspection.rs", min: 2, note: "callers in another crate" },
  { file: "docs/ack-feature-handoff.md", min: 1, note: "Markdown paragraph added by the overlay" },
];
const EXPECTED_NEW_TOTAL = EXPECTED_NEW_SITES.reduce((sum, site) => sum + site.min, 0);

const IDENT = "[A-Za-z0-9_]";
const oldWord = new RegExp(`(?<!${IDENT})${OLD}(?!${IDENT})`, "g");
const newWord = new RegExp(`(?<!${IDENT})${NEW}(?!${IDENT})`, "g");
const newEmbedded = new RegExp(`${IDENT}${NEW}|${NEW}${IDENT}`, "g");

const files = new Map();
for (const root of SCAN_ROOTS) collect(join(worktree, root));
if (files.size === 0) fail(`no .rs or .md files found under ${SCAN_ROOTS.join(", ")} in ${worktree}`);

checkOldNameIsGone();
checkUnrelatedItemsWereNotRenamed();
checkNewNameSites();
checkDefinition();
checkNoDeprecatedAlias();
checkOverlayWorkIsKept();

if (failures.length > 0) {
  console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
  process.exit(1);
}
console.log(
  `PASS: ${OLD} renamed to ${NEW} at every site (base and overlay), unrelated items untouched, signature kept, no alias`,
);

function checkOldNameIsGone() {
  for (const [file, text] of files) {
    for (const match of text.matchAll(oldWord)) {
      if (isUnrelatedOldUse(file, text, match.index)) continue;
      fail(`${file}:${lineOf(text, match.index)}: old name \`${OLD}\` is still used: ${lineText(text, match.index)}`);
    }
  }
}

function isUnrelatedOldUse(file, text, index) {
  const before = text.slice(Math.max(0, index - 200), index);
  return UNRELATED_OLD_USES.some(
    (entry) =>
      entry.file === file &&
      new RegExp(`(?<!${IDENT})${entry.receiver}\\s*\\.\\s*$`).test(before),
  );
}

function checkUnrelatedItemsWereNotRenamed() {
  // The standard-library `PathBuf::as_path` calls must not get the new name.
  const receivers = [...new Set(UNRELATED_OLD_USES.map((entry) => entry.receiver))];
  const wrongReceiver = new RegExp(
    `(?<!${IDENT})(?:${receivers.join("|")})\\s*\\.\\s*${NEW}(?!${IDENT})`,
    "g",
  );
  for (const [file, text] of files) {
    for (const match of text.matchAll(wrongReceiver)) {
      fail(
        `${file}:${lineOf(text, match.index)}: an unrelated \`PathBuf::${OLD}\` call was renamed: ${lineText(text, match.index)}`,
      );
    }
    // An identifier that merely contains the old name (for example the test
    // `directory_junction_treats_shell_metacharacters_as_path_data`) must not change.
    for (const match of text.matchAll(newEmbedded)) {
      fail(
        `${file}:${lineOf(text, match.index)}: an unrelated identifier containing \`${OLD}\` was renamed: ${lineText(text, match.index)}`,
      );
    }
  }
  const junctionTest = "crates/adapters/goldeneye-grammar-pack/tests/materialized_pack.rs";
  const junctionText = files.get(junctionTest);
  if (junctionText !== undefined && !junctionText.includes("shell_metacharacters_as_path_data")) {
    fail(`${junctionTest}: unrelated test name containing \`${OLD}\` was changed`);
  }
  // `xtask/` only has standard-library `as_path` calls; the method is not used there.
  const xtask = new Map();
  collect(join(worktree, "xtask"), xtask);
  for (const [file, text] of xtask) {
    for (const match of text.matchAll(newWord)) {
      fail(
        `${file}:${lineOf(text, match.index)}: an unrelated \`PathBuf::${OLD}\` call was renamed: ${lineText(text, match.index)}`,
      );
    }
  }
}

function checkNewNameSites() {
  let total = 0;
  for (const text of files.values()) total += count(text, newWord);
  for (const site of EXPECTED_NEW_SITES) {
    const text = files.get(site.file);
    if (text === undefined) {
      fail(`${site.file}: file is missing (${site.note})`);
      continue;
    }
    const found = count(text, newWord);
    if (found < site.min) {
      fail(`${site.file}: expected at least ${site.min} use(s) of \`${NEW}\` (${site.note}), found ${found}`);
    }
  }
  if (total < EXPECTED_NEW_TOTAL) {
    fail(`expected at least ${EXPECTED_NEW_TOTAL} uses of \`${NEW}\` under crates/ and docs/, found ${total}`);
  }
}

function checkDefinition() {
  const text = files.get(DEFINITION_FILE);
  if (text === undefined) return;

  const definitions = [];
  const definitionPattern = new RegExp(`(?<!${IDENT})fn\\s+${NEW}(?!${IDENT})`, "g");
  for (const [file, source] of files) {
    if (!file.endsWith(".rs")) continue;
    for (const match of source.matchAll(definitionPattern)) {
      definitions.push(`${file}:${lineOf(source, match.index)}`);
    }
  }
  if (definitions.length !== 1) {
    fail(`expected exactly one definition of \`${NEW}\`, found ${definitions.length}: ${definitions.join(", ")}`);
  }

  const block = implBlock(text, /impl\s+RevalidatedPath\s*\{/);
  if (block === null) {
    fail(`${DEFINITION_FILE}: inherent \`impl RevalidatedPath\` block not found`);
    return;
  }
  // `#[must_use] pub fn <new>(&self) -> &Path { &self.destination }`, doc
  // comments or other attributes may sit between the attribute and the item.
  const signature = new RegExp(
    String.raw`#\[must_use\]\s*(?:(?:\/\/[^\n]*|#\[[^\]]*\])\s*)*pub\s+fn\s+${NEW}\s*\(\s*&\s*self\s*,?\s*\)\s*->\s*&\s*Path\s*\{\s*&\s*self\s*\.\s*destination\s*\}`,
  );
  if (!signature.test(block)) {
    fail(
      `${DEFINITION_FILE}: \`RevalidatedPath::${NEW}\` must keep the original declaration apart from the name: ` +
        "`#[must_use] pub fn " + NEW + "(&self) -> &Path { &self.destination }`",
    );
  }
  if (!/impl\s+AsRef\s*<\s*Path\s*>\s+for\s+RevalidatedPath\s*\{/.test(text)) {
    fail(`${DEFINITION_FILE}: \`impl AsRef<Path> for RevalidatedPath\` was removed`);
  }
}

function checkNoDeprecatedAlias() {
  // The base commit has no `#[deprecated]` attribute anywhere under `crates/`.
  for (const [file, text] of files) {
    if (!file.endsWith(".rs")) continue;
    for (const match of text.matchAll(/#\s*!?\s*\[\s*(?:cfg_attr\s*\([^\]]*)?deprecated\b/g)) {
      fail(`${file}:${lineOf(text, match.index)}: \`#[deprecated]\` item found; no alias may be kept`);
    }
  }
}

function checkOverlayWorkIsKept() {
  const definition = files.get(DEFINITION_FILE);
  if (definition !== undefined && !/pub\s+fn\s+into_path_buf\s*\(\s*self\s*\)\s*->\s*PathBuf/.test(definition)) {
    fail(`${DEFINITION_FILE}: uncommitted work in progress was dropped (\`RevalidatedPath::into_path_buf\`)`);
  }
  const test = files.get(OVERLAY_TEST_FILE);
  if (test !== undefined && !/fn\s+revalidated_update_destination_is_the_canonical_file\s*\(/.test(test)) {
    fail(`${OVERLAY_TEST_FILE}: uncommitted test was dropped or renamed`);
  }
}

function collect(directory, into = files) {
  if (!existsSync(directory)) return;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRECTORIES.has(entry.name)) collect(path, into);
    } else if (entry.isFile() && /\.(?:rs|md)$/.test(entry.name)) {
      into.set(relative(worktree, path).replaceAll("\\", "/"), readFileSync(path, "utf8"));
    }
  }
}

// Returns the text between the braces of the first block opened by `opener`.
function implBlock(text, opener) {
  const match = opener.exec(text);
  if (match === null) return null;
  let depth = 0;
  for (let index = match.index + match[0].length - 1; index < text.length; index += 1) {
    if (text[index] === "{") depth += 1;
    else if (text[index] === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(match.index + match[0].length, index);
    }
  }
  return null;
}

function count(text, pattern) {
  return [...text.matchAll(pattern)].length;
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

function lineText(text, index) {
  const start = text.lastIndexOf("\n", index - 1) + 1;
  const end = text.indexOf("\n", index);
  return text.slice(start, end === -1 ? text.length : end).trim().slice(0, 160);
}

function fail(message) {
  failures.push(message);
}
