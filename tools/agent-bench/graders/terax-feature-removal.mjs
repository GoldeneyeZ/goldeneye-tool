#!/usr/bin/env node
// Grader for the `terax-feature-removal` task.
//
//   node terax-feature-removal.mjs <worktree> <source-repo>
//
// The task removes the code editor's Vim mode completely: the setting and its
// persistence, the settings row, the editor wiring, the module and the npm
// dependency that only Vim mode used, the styling and the callback plumbing
// that existed only for it, and the README bullet. Everything else must keep
// working, including the unrelated code that merely mentions "vim".
//
// 1. Structural checks. Files are parsed with the TypeScript compiler that the
//    target repository already has installed (syntax trees only). What must
//    survive is not hard-coded: it is read from the base commit by the same
//    scan (exports, type members, defaults, strings, mentions of "vim" in
//    unrelated files).
// 2. Integrity checks: every source file parses, no import points at a file
//    that no longer exists, no import or top-level binding of a changed file
//    is left unused.
// 3. Behaviour checks, run in parallel: a held-out Vitest file
//    (fixtures/terax-feature-removal.heldout.ts) plus the repository's
//    own tests of the touched area, and a type-check of the whole project.
//    Type errors and failing repository tests that the base commit already
//    has do not count. A behaviour check that cannot finish in its time
//    budget is skipped with a warning, not failed.
//
// Exit 0 = pass, 1 = fail (failures printed), 2 = the grader could not run.
import { spawn, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmdirSync,
  rmSync,
  symlinkSync,
  unlinkSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BASE = "e7300fb8aff1726f5de77e4f5f84d09613bc89f5";
const PACKAGE = "@replit/codemirror-vim";
const PREF_KEY = "vimMode";
const FEATURE_MODULE = "src/modules/editor/lib/vim.ts";
const STORE = "src/modules/settings/store.ts";
const EXTENSIONS = "src/modules/editor/lib/extensions.ts";
const EDITOR_PANE = "src/modules/editor/EditorPane.tsx";
const EDITOR_STACK = "src/modules/editor/EditorStack.tsx";
const SURFACE = "src/app/components/WorkspaceSurface.tsx";
const APP = "src/app/App.tsx";
const SETTINGS_SECTION = "src/settings/sections/EditorSection.tsx";
const SPACE_SWITCHER = "src/modules/spaces/SpaceSwitcher.tsx";
// The files that implement the feature at the base commit. Every other file
// that mentions "vim" there is unrelated and must keep its mentions.
const FEATURE_FILES = new Set([FEATURE_MODULE, STORE, EXTENSIONS, EDITOR_PANE, SETTINGS_SECTION]);
// `:q` was the only caller of the editor panes' close callback. The task says
// that plumbing which existed only for the feature goes too; set this to
// false to accept solutions that leave the callback chain in place.
const REQUIRE_PLUMBING_REMOVAL = true;
// Repository tests of the touched area (all pass at the base commit).
const REPO_TESTS = [
  "src/modules/settings/coerceFontWeight.test.ts",
  "src/modules/settings/editorFontSize.test.ts",
  "src/modules/editor/lib/eol.test.ts",
  "src/modules/editor/lib/externalFormat.test.ts",
  "src/modules/editor/lib/indent.test.ts",
  "src/modules/editor/lib/languageResolver.test.ts",
  "src/app/eager-budget.test.ts",
];
const HELD_OUT_SOURCE = join(
  dirname(fileURLToPath(import.meta.url)),
  "fixtures",
  "terax-feature-removal.heldout.ts",
);
const HELD_OUT_TARGET = "src/modules/settings/agent-bench-feature-removal.test.ts";
// Time budget: the whole grader stays under 3 minutes.
const TYPECHECK_TIMEOUT_MS = 140_000;
const VITEST_TIMEOUT_MS = 100_000;
const BASELINE_TIMEOUT_MS = 40_000;

const nodeRequire = createRequire(import.meta.url);
const typecheckMode = process.argv[2] === "--typecheck" ? process.argv[3] : null;
const worktree = resolve(process.argv[typecheckMode ? 4 : 2] ?? ".");
const repo = resolve(process.argv[typecheckMode ? 5 : 3] ?? worktree);
const sourceNodeModules = join(repo, "node_modules");

let ts;
try {
  ts = nodeRequire(join(sourceNodeModules, "typescript"));
} catch (error) {
  console.error(`GRADER ERROR: cannot load TypeScript from ${sourceNodeModules}: ${error.message}`);
  process.exit(2);
}

if (typecheckMode) {
  typecheckMain(typecheckMode);
} else {
  main().catch((error) => {
    console.error(`GRADER ERROR: ${error.stack ?? error}`);
    process.exit(2);
  });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const failures = [];
  const warnings = [];
  const fail = (message) => failures.push(message);

  let baseFiles;
  try {
    baseFiles = new Set(git(["ls-tree", "-r", "--name-only", BASE]).split(/\r?\n/).filter(Boolean));
  } catch (error) {
    console.error(`GRADER ERROR: cannot read the base commit ${BASE}: ${error.message}`);
    process.exit(2);
  }
  if (!baseFiles.has(FEATURE_MODULE) || !baseFiles.has(STORE)) {
    console.error(`GRADER ERROR: base commit ${BASE} has no ${FEATURE_MODULE}; wrong repository?`);
    process.exit(2);
  }
  const baseText = (rel) => git(["show", `${BASE}:${rel}`]);
  const baseScans = new Map();
  const baseScan = (rel) => {
    if (!baseScans.has(rel)) baseScans.set(rel, scan(rel, baseText(rel)));
    return baseScans.get(rel);
  };

  const changed = changedPaths();
  const currentFiles = listSourceFiles();
  const current = new Map();
  for (const rel of currentFiles) current.set(rel, scan(rel, readFileSync(join(worktree, rel), "utf8")));

  // --- unrelated code that mentions "vim" at the base commit ---------------
  const unrelated = new Map();
  const mentioning = git(["grep", "-I", "-i", "-l", "-e", "vim", BASE, "--", "src", "src-tauri/src"], [0, 1]);
  for (const line of mentioning.split(/\r?\n/)) {
    const rel = line.slice(line.indexOf(":") + 1);
    if (!rel || FEATURE_FILES.has(rel)) continue;
    unrelated.set(rel, countMatches(baseText(rel), /vim/gi));
  }
  if (unrelated.size < 5) {
    console.error(`GRADER ERROR: only ${unrelated.size} unrelated files mention "vim" at ${BASE}; wrong repository?`);
    process.exit(2);
  }

  // === 1. the feature is gone ==============================================

  if (existsSync(join(worktree, FEATURE_MODULE))) {
    fail(`${FEATURE_MODULE} still exists (only Vim mode used it)`);
  }

  const packageUsers = [];
  const identifiers = [];
  const strings = [];
  const prefUsers = [];
  const cursorRules = [];
  for (const [rel, info] of current) {
    for (const specifier of info.specifiers) {
      if (specifier.text === PACKAGE || specifier.text.startsWith(`${PACKAGE}/`)) {
        packageUsers.push(`${rel}:${specifier.line}`);
      }
    }
    // A test may name the removed feature (to assert that it is gone), and
    // the unrelated files keep their own mentions of "vim".
    if (/\.test\.tsx?$/.test(rel) || unrelated.has(rel)) continue;
    for (const text of info.strings) {
      if (text.text.includes("cm-fat-cursor")) cursorRules.push(`${rel}:${text.line}`);
      if (text.text === PREF_KEY) prefUsers.push(`${rel}:${text.line} "${PREF_KEY}"`);
    }
    for (const [name, lines] of info.identifiers) {
      if (name === PREF_KEY) prefUsers.push(`${rel}:${lines.slice(0, 4).join(",")} \`${name}\``);
    }
    for (const [name, lines] of info.identifiers) {
      if (/vim/i.test(name) && name !== PREF_KEY) identifiers.push(`${rel}:${lines.slice(0, 4).join(",")} \`${name}\``);
    }
    for (const text of info.strings) {
      if (/\bn?vim\b/i.test(text.text) && text.text !== PREF_KEY) {
        strings.push(`${rel}:${text.line} ${JSON.stringify(text.text.trim().slice(0, 60))}`);
      }
    }
  }
  if (packageUsers.length > 0) fail(`"${PACKAGE}" is still imported:\n${bullets(packageUsers)}`);
  if (prefUsers.length > 0) {
    fail(`the \`${PREF_KEY}\` preference is still declared, stored or read:\n${bullets(prefUsers)}`);
  }
  if (identifiers.length > 0) fail(`Vim mode code remains (identifiers):\n${bullets(identifiers)}`);
  if (strings.length > 0) fail(`Vim mode text remains (string literals or JSX text):\n${bullets(strings)}`);
  if (cursorRules.length > 0) {
    fail(`the block-cursor styling that only Vim mode produced (\`.cm-fat-cursor\`) remains:\n${bullets(cursorRules)}`);
  }

  // package.json: the dependency is gone, nothing else changed.
  let manifest = null;
  try {
    manifest = JSON.parse(readFileSync(join(worktree, "package.json"), "utf8"));
  } catch (error) {
    fail(`package.json cannot be read as JSON: ${error.message}`);
  }
  if (manifest) {
    const sections = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];
    for (const section of sections) {
      if (manifest[section] && PACKAGE in manifest[section]) {
        fail(`package.json still lists "${PACKAGE}" in ${section}`);
      }
    }
    const expected = JSON.parse(baseText("package.json"));
    const strip = (value) => {
      const copy = JSON.parse(JSON.stringify(value));
      for (const section of sections) if (copy[section]) delete copy[section][PACKAGE];
      return copy;
    };
    const before = strip(expected);
    const after = strip(manifest);
    const differing = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(
      (key) => stable(before[key]) !== stable(after[key]),
    );
    if (differing.length > 0) {
      const details = differing.map((key) => {
        if (!isRecord(before[key]) || !isRecord(after[key])) return key;
        const inner = [...new Set([...Object.keys(before[key]), ...Object.keys(after[key])])].filter(
          (name) => stable(before[key][name]) !== stable(after[key][name]),
        );
        return `${key}: ${inner.join(", ")}`;
      });
      fail(`package.json changed beyond removing "${PACKAGE}":\n${bullets(details)}`);
    }
  }

  const readme = existsSync(join(worktree, "README.md")) ? readFileSync(join(worktree, "README.md"), "utf8") : null;
  if (readme === null) {
    fail("README.md is missing");
  } else {
    const lines = readme.split(/\r?\n/).filter((line) => /\bvim\b/i.test(line));
    if (lines.length > 0) fail(`README.md still advertises Vim mode:\n${bullets(lines.map((line) => line.trim()))}`);
    const baseReadme = baseText("README.md").split(/\r?\n/).filter((line) => line.trim() && !/\bvim\b/i.test(line));
    const kept = new Set(readme.split(/\r?\n/).map((line) => line.trim()));
    const lost = baseReadme.filter((line) => !kept.has(line.trim()));
    if (lost.length > 0) fail(`README.md lost unrelated lines:\n${bullets(lost.map((line) => line.trim().slice(0, 100)), 6)}`);
  }

  // Plumbing that only Vim's `:q` used.
  if (REQUIRE_PLUMBING_REMOVAL) {
    const leftovers = [];
    const collect = (rel, pattern) => {
      for (const [name, lines] of current.get(rel)?.identifiers ?? []) {
        if (pattern.test(name)) leftovers.push(`${rel}:${lines.slice(0, 4).join(",")} \`${name}\``);
      }
    };
    collect(EDITOR_PANE, /^onClose/);
    collect(EDITOR_STACK, /close/i);
    collect(SURFACE, /^onEditorCloseTab$/);
    collect(APP, /^onEditorCloseTab$/);
    if (leftovers.length > 0) {
      fail(
        `the editor panes' close callback, which only Vim's \`:q\` called, is still plumbed through (dead code):\n${bullets(leftovers)}`,
      );
    }
  }

  // === 2. everything else is intact ========================================

  const requireFile = (rel) => {
    const info = current.get(rel);
    if (!info) fail(`${rel} is missing`);
    return info ?? null;
  };
  const lost = (label, rel, before, after, ignore = () => false) => {
    const missing = [...before].filter((name) => !ignore(name) && !after.has(name));
    if (missing.length > 0) fail(`${rel} lost ${label}: ${missing.slice(0, 12).join(", ")}`);
  };
  const isVim = (name) => /vim/i.test(name);

  // The settings store: every other preference, default, setter and key.
  const store = requireFile(STORE);
  if (store) {
    const before = baseScan(STORE);
    lost("exports", STORE, before.exports, store.exports, isVim);
    lost("`Preferences` members", STORE, typeMembers(before, "Preferences"), typeMembers(store, "Preferences"), isVim);
    const defaultsBefore = objectProperties(before, "DEFAULT_PREFERENCES");
    const defaultsAfter = objectProperties(store, "DEFAULT_PREFERENCES");
    lost("defaults", STORE, new Set(defaultsBefore.keys()), new Set(defaultsAfter.keys()), isVim);
    const changedDefaults = [...defaultsBefore]
      .filter(([name, value]) => defaultsAfter.has(name) && defaultsAfter.get(name) !== value)
      .map(([name]) => name);
    if (changedDefaults.length > 0) fail(`${STORE} changed the defaults of: ${changedDefaults.join(", ")}`);
    lost("string constants (storage keys, event names)", STORE, textSet(before), textSet(store), isVim);
  }

  // The editor's shared extensions: the other compartments and theme rules.
  const extensions = requireFile(EXTENSIONS);
  if (extensions) {
    const before = baseScan(EXTENSIONS);
    lost("exports", EXTENSIONS, before.exports, extensions.exports, isVim);
    lost("theme rules", EXTENSIONS, before.propertyNames, extensions.propertyNames, (name) => name.includes("cm-fat-cursor"));
  }

  // The editor pane: its handle, its props, its other imports and key bindings.
  const pane = requireFile(EDITOR_PANE);
  if (pane) {
    const before = baseScan(EDITOR_PANE);
    lost("exports", EDITOR_PANE, before.exports, pane.exports);
    lost("`EditorPaneHandle` members", EDITOR_PANE, typeMembers(before, "EditorPaneHandle"), typeMembers(pane, "EditorPaneHandle"));
    lost("props", EDITOR_PANE, typeMembers(before, "Props"), typeMembers(pane, "Props"), (name) => name === "onClose");
    lost(
      "imports that other features use",
      EDITOR_PANE,
      before.importedNames,
      pane.importedNames,
      (name) => isVim(name) || name.startsWith(`${PACKAGE}#`),
    );
    lost("key bindings", EDITOR_PANE, new Set(["Tab", "Mod-s", "Ctrl-g"]), textSet(pane));
  }

  // The editor stack and the workspace surface: their other props.
  for (const [rel, removed] of [
    [EDITOR_STACK, "onCloseTab"],
    [SURFACE, "onEditorCloseTab"],
  ]) {
    const info = requireFile(rel);
    if (!info) continue;
    const before = baseScan(rel);
    lost("exports", rel, before.exports, info.exports);
    lost("props", rel, typeMembers(before, "Props"), typeMembers(info, "Props"), (name) => name === removed);
  }

  // The settings page: every other row, label and option.
  const section = requireFile(SETTINGS_SECTION);
  if (section) {
    const before = baseScan(SETTINGS_SECTION);
    lost("exports", SETTINGS_SECTION, before.exports, section.exports);
    // A label of another setting names a Vim command beside the shortcut
    // ("Cmd+S / :w"). Dropping that command from the label is part of the
    // removal, and leaving it is tolerated: compare the labels without it.
    const withoutVimCommands = (texts) =>
      new Set(
        [...texts].map((text) =>
          text.replace(/\s*\/\s*:(?:wq|w|q|x)\b/g, "").replace(/:(?:wq|w|q|x)\b\s*\/\s*/g, ""),
        ),
      );
    lost(
      "settings rows or labels",
      SETTINGS_SECTION,
      withoutVimCommands(textSet(before)),
      withoutVimCommands(textSet(section)),
      (text) => /\bvim\b/i.test(text),
    );
  }

  // Same-named neighbours that are not part of the feature.
  for (const [rel, name] of [
    [APP, "disposeTab"],
    [APP, "handleClose"],
    [APP, "onCloseTab"],
    [SPACE_SWITCHER, "onCloseTab"],
  ]) {
    const info = requireFile(rel);
    if (!info) continue;
    const before = baseScan(rel).identifiers.get(name)?.length ?? 0;
    const allowance = rel === APP && name === "disposeTab" ? 1 : 0;
    const after = info.identifiers.get(name)?.length ?? 0;
    if (after < before - allowance) {
      fail(`${rel}: \`${name}\` is used by other features and must stay (${after} uses, ${before} at the base commit)`);
    }
  }

  // Unrelated code that merely mentions "vim" (file icons, the terminal's
  // list of full-screen programs, the AI prompt, tests, the Rust back end).
  const damaged = [];
  for (const [rel, before] of unrelated) {
    const path = join(worktree, rel);
    if (!existsSync(path)) {
      damaged.push(`${rel} (deleted)`);
      continue;
    }
    const after = countMatches(readFileSync(path, "utf8"), /vim/gi);
    if (after < before) damaged.push(`${rel} (${after} mentions, ${before} at the base commit)`);
  }
  if (damaged.length > 0) {
    fail(`unrelated code that mentions "vim" was removed; it is not part of the editor's Vim mode:\n${bullets(damaged)}`);
  }

  // Paths that the task says to leave alone.
  const touched = (predicate) => changed.filter((entry) => predicate(entry.path));
  const rust = touched((path) => path.startsWith("src-tauri/"));
  if (rust.length > 0) fail(`the back end must not be modified: ${rust.map((entry) => entry.path).join(", ")}`);
  for (const path of ["pnpm-lock.yaml", "ROADMAP.md"]) {
    if (touched((candidate) => candidate === path).length > 0) fail(`${path} must not be modified`);
  }
  const tests = changed.filter((entry) => /\.test\.tsx?$/.test(entry.path) && entry.status !== "A");
  if (tests.length > 0) {
    fail(`existing tests must not be modified or deleted: ${tests.map((entry) => entry.path).join(", ")}`);
  }

  // === 3. integrity ========================================================

  const syntax = [];
  for (const [rel, info] of current) {
    if (info.syntaxErrors.length > 0) syntax.push(`${rel}:${info.syntaxErrors[0]}`);
  }
  if (syntax.length > 0) fail(`files with syntax errors:\n${bullets(syntax)}`);

  const present = new Set(currentFiles);
  const everTracked = new Set([...baseFiles, ...currentFiles]);
  const changedSet = new Set(changed.map((entry) => entry.path));
  const dangling = [];
  for (const [rel, info] of current) {
    for (const specifier of info.specifiers) {
      const target = localTarget(rel, specifier.text);
      if (target === null) continue;
      if (resolves(target, (path) => present.has(path) || existsSync(join(worktree, path)))) continue;
      // Reported when the import is new or edited, or when its target is a
      // file of the base commit that has been deleted.
      if (changedSet.has(rel) || resolves(target, (path) => everTracked.has(path))) {
        dangling.push(`${rel}:${specifier.line} "${specifier.text}"`);
      }
    }
  }
  if (dangling.length > 0) fail(`imports of files that do not exist:\n${bullets(dangling)}`);

  const unused = [];
  for (const entry of changed) {
    const info = current.get(entry.path);
    if (!info) continue;
    for (const name of info.localBindings) {
      if ((info.identifiers.get(name)?.length ?? 0) < 2) unused.push(`${entry.path}: \`${name}\``);
    }
  }
  if (unused.length > 0) fail(`unused imports or top-level declarations left behind:\n${bullets(unused)}`);

  // === 4. behaviour: held-out test, repository tests, type-check ===========

  let testNote = "tests not run (structural checks failed)";
  let typecheckNote = "type-check not run (structural checks failed)";
  if (failures.length === 0) {
    const nodeModules = join(worktree, "node_modules");
    const heldOut = join(worktree, HELD_OUT_TARGET);
    let linked = false;
    let copied = false;
    try {
      if (!existsSync(nodeModules)) {
        symlinkSync(sourceNodeModules, nodeModules, process.platform === "win32" ? "junction" : "dir");
        linked = true;
      }
      if (existsSync(heldOut)) {
        fail(`${HELD_OUT_TARGET} already exists in the worktree`);
      } else {
        copyFileSync(HELD_OUT_SOURCE, heldOut);
        copied = true;
        [testNote, typecheckNote] = await Promise.all([runTests(fail, warnings), runTypecheck(fail, warnings)]);
      }
    } finally {
      if (copied) rmSync(heldOut, { force: true });
      if (linked) unlink(nodeModules);
    }
  }

  for (const warning of warnings) console.error(`WARN: ${warning}`);
  if (failures.length > 0) {
    console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
    process.exit(1);
  }
  console.log(
    `PASS: Vim mode removed, neighbours intact (${unrelated.size} unrelated files that mention "vim" untouched); ${testNote}; ${typecheckNote}`,
  );
}

// ---------------------------------------------------------------------------
// Behaviour checks
// ---------------------------------------------------------------------------

async function runTests(fail, warnings) {
  const vitest = join(sourceNodeModules, "vitest", "vitest.mjs");
  if (!existsSync(vitest)) {
    fail(`Vitest runtime not found at ${vitest}`);
    return "tests not run";
  }
  const repoTests = REPO_TESTS.filter((rel) => existsSync(join(worktree, rel)));
  const missing = REPO_TESTS.filter((rel) => !repoTests.includes(rel));
  if (missing.length > 0) fail(`repository tests are missing: ${missing.join(", ")}`);

  const run = await vitestRun(vitest, worktree, [HELD_OUT_TARGET, ...repoTests], VITEST_TIMEOUT_MS);
  if (run.timedOut) {
    warnings.push(`tests skipped: did not finish within ${VITEST_TIMEOUT_MS / 1000}s`);
    return "tests SKIPPED (timed out)";
  }
  if (!run.results) {
    fail(`the tests could not be run:\n${tail(run.output)}`);
    return "tests failed";
  }
  const heldOut = run.results(HELD_OUT_TARGET);
  if (!heldOut || heldOut.status !== "passed" || heldOut.tests === 0) {
    fail(`held-out test failed:\n${bullets(heldOut?.messages ?? [tail(run.output)], 12)}`);
  }
  const failed = repoTests.filter((rel) => run.results(rel)?.status !== "passed");
  let alreadyFailing = [];
  if (failed.length > 0 && resolve(repo) !== resolve(worktree)) {
    const baseline = await vitestRun(vitest, repo, failed, BASELINE_TIMEOUT_MS);
    if (baseline.results) {
      alreadyFailing = failed.filter((rel) => baseline.results(rel)?.status !== "passed");
    }
  }
  const broken = failed.filter((rel) => !alreadyFailing.includes(rel));
  if (alreadyFailing.length > 0) {
    warnings.push(`repository tests that also fail in ${repo} are ignored: ${alreadyFailing.join(", ")}`);
  }
  if (broken.length > 0) {
    fail(
      `repository tests fail:\n${bullets(
        broken.flatMap((rel) => [rel, ...(run.results(rel)?.messages ?? []).slice(0, 3)]),
        12,
      )}`,
    );
  }
  return `held-out test (${heldOut?.tests ?? 0} cases) and ${repoTests.length - broken.length}/${repoTests.length} repository test files pass`;
}

async function vitestRun(vitest, root, files, timeoutMs) {
  const reportDir = mkdtempSync(join(tmpdir(), "terax-feature-removal-"));
  const report = join(reportDir, "report.json");
  try {
    const run = await runProcess(
      [vitest, "run", "--root", root, "--reporter=json", `--outputFile=${report}`, ...files],
      root,
      timeoutMs,
    );
    if (run.timedOut) return { timedOut: true };
    let parsed;
    try {
      parsed = JSON.parse(readFileSync(report, "utf8"));
    } catch {
      return { results: null, output: run.stderr || run.stdout };
    }
    const byName = new Map();
    for (const file of parsed.testResults ?? []) {
      const name = String(file.name).replace(/\\/g, "/").toLowerCase();
      const assertions = file.assertionResults ?? [];
      const messages = [];
      if (file.message) messages.push(firstLines(file.message));
      for (const assertion of assertions) {
        if (assertion.status === "passed") continue;
        messages.push(`${assertion.fullName}: ${firstLines((assertion.failureMessages ?? []).join(" "))}`);
      }
      byName.set(name, { status: file.status, tests: assertions.length, messages });
    }
    // Vitest reports absolute paths; a test file is looked up by its path
    // inside the repository.
    const results = (rel) => {
      const suffix = `/${rel.toLowerCase()}`;
      for (const [name, result] of byName) if (name.endsWith(suffix)) return result;
      return undefined;
    };
    return { results, output: run.stderr || run.stdout };
  } finally {
    rmSync(reportDir, { force: true, recursive: true });
  }
}

async function runTypecheck(fail, warnings) {
  const self = fileURLToPath(import.meta.url);
  const child = (mode) => runProcess([self, "--typecheck", mode, worktree, repo], worktree, TYPECHECK_TIMEOUT_MS);
  // Two compilations side by side: the worktree as it is, and the worktree as
  // the base commit saw it. Only errors that the base commit does not have count.
  const [now, before] = await Promise.all([child("current"), child("base")]);
  if (now.timedOut || before.timedOut) {
    warnings.push(`type-check skipped: did not finish within ${TYPECHECK_TIMEOUT_MS / 1000}s`);
    return "type-check SKIPPED (timed out)";
  }
  let currentErrors;
  let baseErrors;
  try {
    currentErrors = JSON.parse(now.stdout);
    baseErrors = JSON.parse(before.stdout);
  } catch {
    warnings.push(`type-check skipped: the compiler run failed:\n${tail(now.stderr || before.stderr || now.stdout)}`);
    return "type-check SKIPPED (compiler run failed)";
  }
  const known = new Set(baseErrors.errors.map((error) => error.id));
  const fresh = currentErrors.errors
    .filter((error) => !known.has(error.id) && !error.id.startsWith(`${HELD_OUT_TARGET}|`))
    .map((error) => error.text);
  if (fresh.length > 0) {
    fail(
      `${fresh.length} new type error(s) (${known.size} already present at the base commit are ignored):\n${bullets(fresh, 20)}`,
    );
    return "type-check failed";
  }
  return `type-check clean (${currentErrors.errors.length} errors, none new; ${currentErrors.files} files)`;
}

function runProcess(args, cwd, timeoutMs) {
  return new Promise((done) => {
    const child = spawn(process.execPath, args, { cwd, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    child.stdout.setEncoding("utf8").on("data", (chunk) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk) => (stderr += chunk));
    const timer = setTimeout(() => {
      timedOut = true;
      if (process.platform === "win32") {
        spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { windowsHide: true });
      } else {
        child.kill("SIGKILL");
      }
    }, timeoutMs);
    child.on("error", (error) => {
      clearTimeout(timer);
      done({ status: null, stdout, stderr: `${stderr}\n${error.message}`, timedOut });
    });
    child.on("close", (status) => {
      clearTimeout(timer);
      done({ status, stdout, stderr, timedOut });
    });
  });
}

// ---------------------------------------------------------------------------
// Syntax scan
// ---------------------------------------------------------------------------

// One pass over a file: identifiers, string and JSX text, module specifiers,
// exports, imported names, string-named properties, imports and top-level
// declarations that are not exported.
function scan(rel, text) {
  const kind = rel.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const source = ts.createSourceFile(rel, text, ts.ScriptTarget.Latest, true, kind);
  const lineOf = (node) => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const info = {
    source,
    identifiers: new Map(),
    strings: [],
    specifiers: [],
    exports: new Set(),
    importedNames: new Set(),
    propertyNames: new Set(),
    localBindings: new Set(),
    syntaxErrors: (source.parseDiagnostics ?? []).map(
      (diagnostic) =>
        `${source.getLineAndCharacterOfPosition(diagnostic.start ?? 0).line + 1} ${ts.flattenDiagnosticMessageText(diagnostic.messageText, " ")}`,
    ),
  };
  const exported = (node) => (ts.getCombinedModifierFlags(node) & ts.ModifierFlags.Export) !== 0;
  const bindingNames = (name, out) => {
    if (ts.isIdentifier(name)) out.push(name.text);
    else for (const element of name.elements) if (!ts.isOmittedExpression(element)) bindingNames(element.name, out);
  };

  for (const statement of source.statements) {
    if (ts.isImportDeclaration(statement)) {
      const module = statement.moduleSpecifier.text;
      const clause = statement.importClause;
      if (clause?.name) {
        info.localBindings.add(clause.name.text);
        info.importedNames.add(`${module}#default`);
      }
      const bindings = clause?.namedBindings;
      if (bindings && ts.isNamespaceImport(bindings)) {
        info.localBindings.add(bindings.name.text);
        info.importedNames.add(`${module}#*`);
      } else if (bindings) {
        for (const element of bindings.elements) {
          info.localBindings.add(element.name.text);
          info.importedNames.add(`${module}#${(element.propertyName ?? element.name).text}`);
        }
      }
    } else if (ts.isVariableStatement(statement)) {
      const names = [];
      for (const declaration of statement.declarationList.declarations) bindingNames(declaration.name, names);
      for (const name of names) (exported(statement) ? info.exports : info.localBindings).add(name);
    } else if (
      (ts.isFunctionDeclaration(statement) ||
        ts.isClassDeclaration(statement) ||
        ts.isTypeAliasDeclaration(statement) ||
        ts.isInterfaceDeclaration(statement) ||
        ts.isEnumDeclaration(statement)) &&
      statement.name
    ) {
      (exported(statement) ? info.exports : info.localBindings).add(statement.name.text);
    } else if (ts.isExportDeclaration(statement) && statement.exportClause && ts.isNamedExports(statement.exportClause)) {
      for (const element of statement.exportClause.elements) info.exports.add(element.name.text);
    }
  }
  // A name exported through `export { name }` is not a leftover.
  for (const name of info.exports) info.localBindings.delete(name);

  const visit = (node) => {
    if (ts.isIdentifier(node)) {
      const lines = info.identifiers.get(node.text) ?? [];
      lines.push(lineOf(node));
      info.identifiers.set(node.text, lines);
    } else if (ts.isStringLiteralLike(node) || ts.isJsxText(node) || ts.isTemplateLiteralToken(node)) {
      if (node.text.trim()) info.strings.push({ text: node.text, line: lineOf(node) });
      const parent = node.parent;
      if (ts.isStringLiteralLike(node)) {
        if (
          ((ts.isImportDeclaration(parent) || ts.isExportDeclaration(parent)) && parent.moduleSpecifier === node) ||
          (ts.isCallExpression(parent) &&
            parent.arguments[0] === node &&
            (parent.expression.kind === ts.SyntaxKind.ImportKeyword ||
              (ts.isIdentifier(parent.expression) && parent.expression.text === "require"))) ||
          (ts.isLiteralTypeNode(parent) && ts.isImportTypeNode(parent.parent))
        ) {
          info.specifiers.push({ text: node.text, line: lineOf(node) });
        }
        if ((ts.isPropertyAssignment(parent) || ts.isPropertySignature(parent)) && parent.name === node) {
          info.propertyNames.add(node.text);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return info;
}

function textSet(info) {
  return new Set(info.strings.map((entry) => entry.text.trim().replace(/\s+/g, " ")));
}

// Member names of `type Name = { ... }` or `interface Name { ... }`.
function typeMembers(info, name) {
  const members = new Set();
  for (const statement of info.source.statements) {
    let list = null;
    if (ts.isTypeAliasDeclaration(statement) && statement.name.text === name && ts.isTypeLiteralNode(statement.type)) {
      list = statement.type.members;
    } else if (ts.isInterfaceDeclaration(statement) && statement.name.text === name) {
      list = statement.members;
    }
    for (const member of list ?? []) {
      if (member.name && (ts.isIdentifier(member.name) || ts.isStringLiteralLike(member.name))) members.add(member.name.text);
    }
  }
  return members;
}

// Property names and initializer texts of `const NAME = { ... }`.
function objectProperties(info, name) {
  const properties = new Map();
  for (const statement of info.source.statements) {
    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name) || declaration.name.text !== name) continue;
      let value = declaration.initializer;
      while (value && (ts.isAsExpression(value) || ts.isSatisfiesExpression(value) || ts.isParenthesizedExpression(value))) {
        value = value.expression;
      }
      if (!value || !ts.isObjectLiteralExpression(value)) continue;
      for (const property of value.properties) {
        if (!property.name || !(ts.isIdentifier(property.name) || ts.isStringLiteralLike(property.name))) continue;
        const initializer = ts.isPropertyAssignment(property) ? property.initializer.getText(info.source) : "";
        properties.set(property.name.text, initializer.replace(/\s+/g, ""));
      }
    }
  }
  return properties;
}

// The repository path that a relative or `@/` specifier points at, or null
// for packages.
function localTarget(rel, specifier) {
  const bare = specifier.replace(/[?#].*$/, "");
  if (bare.startsWith("@/")) return posix.normalize(`src/${bare.slice(2)}`);
  if (bare.startsWith("./") || bare.startsWith("../") || bare === "." || bare === "..") {
    return posix.normalize(posix.join(posix.dirname(rel), bare));
  }
  return null;
}

function resolves(target, exists) {
  const candidates = [
    target,
    ...[".ts", ".tsx", ".d.ts", ".mts", ".js", ".jsx", ".mjs", ".json", ".css"].map((suffix) => target + suffix),
    ...["/index.ts", "/index.tsx", "/index.js"].map((suffix) => target + suffix),
    target.replace(/\.js$/, ".ts"),
    target.replace(/\.js$/, ".tsx"),
    target.replace(/\.mjs$/, ".mts"),
  ];
  return candidates.some((candidate) => exists(candidate));
}

function listSourceFiles() {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(join(worktree, dir), { withFileTypes: true })) {
      const rel = `${dir}/${entry.name}`;
      if (entry.isSymbolicLink() || entry.name === "node_modules") continue;
      if (entry.isDirectory()) walk(rel);
      else if (entry.isFile() && /\.tsx?$/.test(rel) && !rel.endsWith(".d.ts")) out.push(rel);
    }
  };
  if (existsSync(join(worktree, "src"))) walk("src");
  return out;
}

// ---------------------------------------------------------------------------
// Type-check (child process: `--typecheck current|base <worktree> <repo>`)
// ---------------------------------------------------------------------------

function typecheckMain(mode) {
  const configPath = join(worktree, "tsconfig.json");
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  const parsed = ts.parseJsonConfigFileContent(config.config ?? {}, ts.sys, worktree, undefined, configPath);
  const options = { ...parsed.options, noEmit: true, incremental: false, composite: false };
  const key = (path) => path.replace(/\\/g, "/").toLowerCase();
  const root = key(worktree);
  const relative = (path) => (key(path).startsWith(`${root}/`) ? path.replace(/\\/g, "/").slice(root.length + 1) : path);
  const describe = (diagnostic) => {
    const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, " ");
    const file = diagnostic.file ? relative(diagnostic.file.fileName) : "<config>";
    const line = diagnostic.file ? diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start ?? 0).line + 1 : 0;
    return { id: `${file}|TS${diagnostic.code}|${message}`, text: `${file}:${line} TS${diagnostic.code} ${message}` };
  };
  const errorsOf = (program, extra = []) =>
    [...extra, ...ts.getPreEmitDiagnostics(program)]
      .filter((diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error)
      .map(describe);

  let program;
  let extra = [];
  if (mode === "current") {
    program = ts.createProgram({ rootNames: parsed.fileNames, options });
    extra = [...(config.error ? [config.error] : []), ...parsed.errors];
  } else {
    // The program as the base commit saw it: every path that differs from the
    // base commit is served from git instead of from disk.
    const overlay = new Map();
    const added = new Set();
    const restored = [];
    for (const entry of changedPaths()) {
      const absolute = key(join(worktree, entry.path));
      if (entry.status === "A") {
        overlay.set(absolute, null);
        added.add(absolute);
      } else {
        overlay.set(absolute, git(["show", `${BASE}:${entry.path}`]));
        if (entry.status === "D" && /^src\/.*\.tsx?$/.test(entry.path)) {
          restored.push(join(worktree, entry.path).replace(/\\/g, "/"));
        }
      }
    }
    const host = ts.createCompilerHost(options, true);
    const fileExists = host.fileExists.bind(host);
    const readFile = host.readFile.bind(host);
    const getSourceFile = host.getSourceFile.bind(host);
    host.fileExists = (path) => (overlay.has(key(path)) ? overlay.get(key(path)) !== null : fileExists(path));
    host.readFile = (path) => (overlay.has(key(path)) ? (overlay.get(key(path)) ?? undefined) : readFile(path));
    host.getSourceFile = (path, languageVersion, onError, shouldCreate) => {
      if (!overlay.has(key(path))) return getSourceFile(path, languageVersion, onError, shouldCreate);
      const text = overlay.get(key(path));
      return text === null ? undefined : ts.createSourceFile(path, text, languageVersion, true);
    };
    const rootNames = [...parsed.fileNames.filter((path) => !added.has(key(path))), ...restored];
    program = ts.createProgram({ rootNames, options, host });
  }
  process.stdout.write(JSON.stringify({ files: parsed.fileNames.length, errors: errorsOf(program, extra) }));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Paths that differ from the base commit: status "A" (added or untracked),
// "D" (deleted) or "M".
function changedPaths() {
  const entries = [];
  for (const line of git(["diff", "--name-status", "--no-renames", BASE, "--", "."]).split(/\r?\n/)) {
    const [status, path] = line.split("\t");
    if (path) entries.push({ status: status[0] === "A" || status[0] === "D" ? status[0] : "M", path });
  }
  for (const path of git(["ls-files", "--others", "--exclude-standard"]).split(/\r?\n/)) {
    if (path && path !== HELD_OUT_TARGET) entries.push({ status: "A", path });
  }
  return entries.filter((entry) => entry.path !== HELD_OUT_TARGET);
}

function git(args, okStatuses = [0]) {
  const result = spawnSync("git", ["-C", worktree, "-c", "core.quotepath=off", ...args], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error || !okStatuses.includes(result.status)) {
    throw new Error(`git ${args.join(" ")} failed: ${tail(result.stderr || String(result.error ?? ""))}`);
  }
  return result.stdout ?? "";
}

// Removes a link to the source repository's node_modules, never its contents.
function unlink(path) {
  let stat;
  try {
    stat = lstatSync(path);
  } catch {
    return;
  }
  if (!stat.isSymbolicLink()) return;
  try {
    unlinkSync(path);
  } catch {
    try {
      rmdirSync(path);
    } catch {}
  }
}

function countMatches(text, pattern) {
  return (text.match(pattern) ?? []).length;
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (isRecord(value)) {
    return `{${Object.keys(value)
      .sort()
      .map((name) => `${JSON.stringify(name)}:${stable(value[name])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function firstLines(value, count = 4) {
  return String(value ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, count)
    .join(" | ")
    .slice(0, 400);
}

function bullets(items, limit = 15) {
  const shown = items.slice(0, limit).map((item) => `   - ${item}`);
  if (items.length > limit) shown.push(`   - ... and ${items.length - limit} more`);
  return shown.join("\n");
}

function tail(value) {
  return String(value ?? "").slice(-4000);
}
