#!/usr/bin/env node
// Grader for the `terax-module-refactor` task.
//
//   node terax-module-refactor.mjs <worktree> <source-repo>
//
// The task moves `src/modules/ai/lib/native.ts` to `src/lib/nativeBridge.ts`
// and renames its exported object `native` to `nativeBridge`, updating every
// import, call and test mock, with no shim left behind.
//
// 1. Structural checks. The files are parsed with the TypeScript compiler
//    that the target repository already has installed (no type information,
//    syntax trees only). The expected numbers of usages are not hard-coded:
//    they are counted from the base commit by the same scan.
// 2. Behaviour checks, only when the structure is right: the existing tests
//    that touch the module must pass, and the whole project is type-checked.
//    Type errors that the base commit already has do not count. Either
//    check is skipped with a warning when it cannot finish in time.
//
// Exit 0 = pass, 1 = fail (failures printed), 2 = the grader could not run.
import { spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BASE = "e7300fb8aff1726f5de77e4f5f84d09613bc89f5";
const OLD_MODULE = "src/modules/ai/lib/native";
const NEW_MODULE = "src/lib/nativeBridge";
const NEW_SPECIFIER = "@/lib/nativeBridge";
const OLD_NAME = "native";
const NEW_NAME = "nativeBridge";
const MIN_BASE_REFERENCES = 15;
const MIN_BASE_FILES = 8;
// Time budget: the whole grader stays under 3 minutes. A check that cannot
// finish in its share (cold disk cache) is skipped with a warning, not failed.
const DEADLINE_MS = 165_000;
const TYPECHECK_TIMEOUT_MS = 115_000;
const VITEST_TIMEOUT_MS = 75_000;
const MOCK_METHODS = new Set([
  "mock",
  "doMock",
  "unmock",
  "doUnmock",
  "importActual",
  "importMock",
]);

const nodeRequire = createRequire(import.meta.url);
const typecheckMode = process.argv[2] === "--typecheck";
const worktree = resolve(process.argv[typecheckMode ? 3 : 2] ?? ".");
const repo = resolve(process.argv[typecheckMode ? 4 : 3] ?? worktree);
const sourceNodeModules = join(repo, "node_modules");

let ts;
try {
  ts = nodeRequire(join(sourceNodeModules, "typescript"));
} catch (error) {
  console.error(
    `GRADER ERROR: cannot load TypeScript from ${sourceNodeModules}: ${error.message}`,
  );
  process.exit(2);
}

if (typecheckMode) {
  typecheckMain();
} else {
  main();
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const started = Date.now();
  const failures = [];
  const warnings = [];
  const fail = (message) => failures.push(message);

  // --- what the base commit looks like ------------------------------------
  let base;
  try {
    base = scanBase();
  } catch (error) {
    console.error(`GRADER ERROR: cannot read the base commit ${BASE}: ${error.message}`);
    process.exit(2);
  }
  if (base.files.size < MIN_BASE_FILES || base.references < MIN_BASE_REFERENCES) {
    console.error(
      `GRADER ERROR: base commit has only ${base.references} references to ${OLD_MODULE} in ${base.files.size} files; wrong repository?`,
    );
    process.exit(2);
  }

  // --- the old module must be gone ----------------------------------------
  for (const suffix of [".ts", ".tsx", ".d.ts", ".js", "/index.ts", "/index.tsx"]) {
    if (existsSync(join(worktree, OLD_MODULE + suffix))) {
      fail(`old module still exists: ${OLD_MODULE}${suffix} (it must be deleted, not kept as a shim)`);
    }
  }

  // --- the new module ------------------------------------------------------
  const newFile = `${NEW_MODULE}.ts`;
  let moved = null;
  if (!existsSync(join(worktree, newFile))) {
    fail(`new module is missing: ${newFile}`);
  } else {
    moved = scanModule(newFile, readFileSync(join(worktree, newFile), "utf8"), NEW_NAME);
    if (!moved.exports.has(NEW_NAME)) {
      fail(`${newFile} does not export \`${NEW_NAME}\``);
    } else if (!moved.object) {
      fail(`${newFile} must define \`${NEW_NAME}\` as an object literal (not an alias of something else)`);
    }
    if (moved.exports.has(OLD_NAME)) {
      fail(`${newFile} still exports the old name \`${OLD_NAME}\``);
    }
    if (moved.reexports) {
      fail(`${newFile} re-exports from another module; the bridge must live in this file`);
    }
    if (moved.oldIdentifiers.length > 0) {
      fail(`${newFile} still uses the identifier \`${OLD_NAME}\` (line ${moved.oldIdentifiers.slice(0, 5).join(", ")})`);
    }
    const missingExports = [...base.module.exports].filter(
      (name) => name !== OLD_NAME && !moved.exports.has(name),
    );
    if (missingExports.length > 0) {
      fail(`${newFile} no longer exports: ${missingExports.join(", ")}`);
    }
    if (moved.object) {
      const missingKeys = [...base.module.keys.keys()].filter((key) => !moved.keys.has(key));
      if (missingKeys.length > 0) {
        fail(`\`${NEW_NAME}\` lost methods: ${missingKeys.join(", ")}`);
      }
      const changed = [...base.module.keys]
        .filter(([key, signature]) => moved.keys.has(key) && moved.keys.get(key) !== signature)
        .map(([key]) => key);
      if (changed.length > 0) {
        fail(`\`${NEW_NAME}\` methods changed their parameters: ${changed.join(", ")}`);
      }
    }
    const missingCommands = [...base.module.commands].filter(
      (command) => !moved.commands.has(command),
    );
    if (missingCommands.length > 0) {
      fail(`${newFile} no longer invokes backend commands: ${missingCommands.join(", ")}`);
    }
  }

  // --- every usage in src/ -------------------------------------------------
  const current = new Map();
  for (const rel of listSourceFiles()) {
    current.set(rel, scanFile(rel, readFileSync(join(worktree, rel), "utf8")));
  }

  const oldReferences = [];
  const wrongSpecifiers = [];
  const shims = [];
  const badImports = [];
  const unknownImports = [];
  const unknownMembers = [];
  const badMocks = [];
  const leftovers = [];
  let references = 0;
  let valueImporters = 0;
  let memberUses = 0;
  let mockSites = 0;

  for (const [rel, info] of current) {
    for (const literal of info.literals) {
      if (literal.id === OLD_MODULE) oldReferences.push(`${rel}:${literal.line} "${literal.text}"`);
      if (literal.id === NEW_MODULE) {
        references += 1;
        if (literal.text !== NEW_SPECIFIER) {
          wrongSpecifiers.push(`${rel}:${literal.line} "${literal.text}"`);
        }
      }
    }
    for (const reexport of info.reexports) {
      if (reexport.id === NEW_MODULE || reexport.id === OLD_MODULE) {
        shims.push(`${rel}:${reexport.line}`);
      }
    }
    const locals = [];
    for (const declaration of info.imports) {
      if (declaration.id !== NEW_MODULE) continue;
      for (const binding of declaration.bindings) {
        if (binding.imported === OLD_NAME || binding.local === OLD_NAME) {
          badImports.push(`${rel}:${declaration.line} imports \`${OLD_NAME}\``);
        } else if (binding.imported === NEW_NAME && binding.local !== NEW_NAME) {
          badImports.push(`${rel}:${declaration.line} aliases \`${NEW_NAME}\` as \`${binding.local}\``);
        } else if (binding.imported === NEW_NAME) {
          locals.push(binding.local);
        }
        if (
          moved &&
          binding.imported !== "*" &&
          binding.imported !== "default" &&
          !moved.exports.has(binding.imported)
        ) {
          unknownImports.push(`${rel}:${declaration.line} \`${binding.imported}\``);
        }
      }
    }
    if (locals.length > 0) valueImporters += 1;
    for (const member of info.members) {
      if (!locals.includes(member.object)) continue;
      memberUses += 1;
      if (moved?.object && !moved.keys.has(member.member)) {
        unknownMembers.push(`${rel}:${member.line} \`${NEW_NAME}.${member.member}\``);
      }
    }
    for (const mock of info.mocks) {
      if (mock.id !== NEW_MODULE) continue;
      mockSites += 1;
      if (mock.hasFactory && (!mock.keys.has(NEW_NAME) || mock.keys.has(OLD_NAME))) {
        badMocks.push(`${rel}:${mock.line}`);
      }
    }
    const usesBridge =
      base.files.has(rel) || info.literals.some((literal) => literal.id === NEW_MODULE);
    if (usesBridge && info.oldIdentifiers.length > 0) {
      leftovers.push(`${rel}:${info.oldIdentifiers.slice(0, 6).join(",")}`);
    }
  }

  if (oldReferences.length > 0) {
    fail(`${oldReferences.length} reference(s) to the old module path remain:\n${bullets(oldReferences)}`);
  }
  if (wrongSpecifiers.length > 0) {
    fail(`the new module must be referenced as "${NEW_SPECIFIER}":\n${bullets(wrongSpecifiers)}`);
  }
  if (shims.length > 0) {
    fail(`re-export shim(s) of the bridge module:\n${bullets(shims)}`);
  }
  if (badImports.length > 0) {
    fail(`old name still imported or aliased:\n${bullets(badImports)}`);
  }
  if (leftovers.length > 0) {
    fail(`identifier \`${OLD_NAME}\` remains in files that use the bridge (file:lines):\n${bullets(leftovers)}`);
  }
  if (unknownImports.length > 0) {
    fail(`imports of names that ${newFile} does not export:\n${bullets(unknownImports)}`);
  }
  if (unknownMembers.length > 0) {
    fail(`calls to methods that \`${NEW_NAME}\` does not have:\n${bullets(unknownMembers)}`);
  }
  if (badMocks.length > 0) {
    fail(`test mocks of the module must provide \`${NEW_NAME}\` (and not \`${OLD_NAME}\`):\n${bullets(badMocks)}`);
  }

  const notMigrated = [];
  for (const rel of base.files.keys()) {
    const info = current.get(rel);
    if (!info) notMigrated.push(`${rel} (file removed)`);
    else if (!info.literals.some((literal) => literal.id === NEW_MODULE)) notMigrated.push(rel);
  }
  if (notMigrated.length > 0) {
    fail(`${notMigrated.length} of ${base.files.size} files that used the module do not reference "${NEW_SPECIFIER}":\n${bullets(notMigrated)}`);
  }
  requireAtLeast("references to the new module", references, base.references);
  requireAtLeast(`files importing \`${NEW_NAME}\``, valueImporters, base.valueImporters);
  requireAtLeast(`member uses \`${NEW_NAME}.<method>\``, memberUses, base.memberUses);
  requireAtLeast("test mocks of the new module", mockSites, base.mockSites);

  const touched = changedPaths();
  const rust = touched.filter((path) => path.endsWith(".rs"));
  if (rust.length > 0) fail(`Rust files must not be modified: ${rust.join(", ")}`);

  // --- behaviour: tests and type-check -------------------------------------
  let typecheckNote = "type-check not run (structural checks failed)";
  let testNote = "tests not run (structural checks failed)";
  if (failures.length === 0) {
    const nodeModules = join(worktree, "node_modules");
    let linked = false;
    try {
      if (!existsSync(nodeModules)) {
        symlinkSync(sourceNodeModules, nodeModules, process.platform === "win32" ? "junction" : "dir");
        linked = true;
      }
      const remaining = () => DEADLINE_MS - (Date.now() - started);
      typecheckNote = runTypecheck(Math.min(TYPECHECK_TIMEOUT_MS, remaining()), fail, warnings);
      testNote = runTests(
        [...base.files.keys()].filter((rel) => /\.test\.tsx?$/.test(rel)),
        Math.min(VITEST_TIMEOUT_MS, remaining()),
        fail,
        warnings,
      );
    } finally {
      if (linked && lstatSync(nodeModules).isSymbolicLink()) {
        rmSync(nodeModules, { force: true, recursive: true });
      }
    }
  }

  const counts =
    `references ${references}/${base.references}, importing files ${valueImporters}/${base.valueImporters}, ` +
    `member uses ${memberUses}/${base.memberUses}, mocks ${mockSites}/${base.mockSites} (found/expected from base)`;
  for (const warning of warnings) console.error(`WARN: ${warning}`);
  if (failures.length > 0) {
    console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
    console.error(`counts: ${counts}`);
    process.exit(1);
  }
  console.log(`PASS: module moved and renamed everywhere; ${counts}; ${testNote}; ${typecheckNote}`);

  function requireAtLeast(label, actual, expected) {
    if (actual < expected) fail(`${label}: found ${actual}, the base commit has ${expected}`);
  }
}

function runTests(testFiles, timeoutMs, fail, warnings) {
  const vitest = join(sourceNodeModules, "vitest", "vitest.mjs");
  const existing = testFiles.filter((rel) => existsSync(join(worktree, rel)));
  if (!existsSync(vitest)) {
    fail(`Vitest runtime not found at ${vitest}`);
    return "tests not run";
  }
  if (existing.length === 0) return "no tests to run";
  if (timeoutMs < 15_000) {
    warnings.push("tests skipped: no time left in the grader budget");
    return "tests SKIPPED (no time left)";
  }
  const startedAt = Date.now();
  const current = failingTestFiles(vitest, worktree, existing, timeoutMs);
  if (current.timedOut) {
    warnings.push(`tests skipped: did not finish within ${Math.round(timeoutMs / 1000)}s`);
    return "tests SKIPPED (timed out)";
  }
  if (current.failed === null) {
    fail(`existing tests that use the module could not be run (${existing.join(", ")}):\n${current.output}`);
    return "tests failed";
  }
  if (current.failed.length === 0) return `${existing.length} test files passed`;

  // A test file that already fails at the base commit (a dependency missing
  // from the local install, for example) says nothing about the change.
  let alreadyFailing = [];
  const baseFiles = existing.filter((rel) => existsSync(join(repo, rel)));
  const remaining = timeoutMs - (Date.now() - startedAt);
  if (resolve(repo) !== resolve(worktree) && baseFiles.length > 0 && remaining >= 15_000) {
    const baseline = failingTestFiles(vitest, repo, baseFiles, remaining);
    alreadyFailing = current.failed.filter((rel) => baseline.failed?.includes(rel));
  }
  if (alreadyFailing.length > 0) {
    warnings.push(
      `${alreadyFailing.length} test file(s) already fail at the base commit and are not counted: ${alreadyFailing.join(", ")}`,
    );
  }
  const broken = current.failed.filter((rel) => !alreadyFailing.includes(rel));
  if (broken.length > 0) {
    fail(`existing tests that use the module failed (${broken.join(", ")}):\n${current.output}`);
    return "tests failed";
  }
  return `${existing.length - alreadyFailing.length} test files passed, ${alreadyFailing.length} already failing at base`;
}

// The test files, among `files`, that do not pass under `root`; `failed` is
// null when the run produced no report.
function failingTestFiles(vitest, root, files, timeoutMs) {
  const reportDir = mkdtempSync(join(tmpdir(), "terax-grader-"));
  const reportFile = join(reportDir, "vitest.json");
  try {
    const run = spawnSync(
      process.execPath,
      [vitest, "run", "--root", root, "--reporter=default", "--reporter=json", `--outputFile.json=${reportFile}`, ...files],
      { cwd: root, encoding: "utf8", maxBuffer: 16 * 1024 * 1024, timeout: timeoutMs, windowsHide: true },
    );
    if (run.error?.code === "ETIMEDOUT" || run.signal) return { timedOut: true };
    const output = tail(`${run.stdout ?? ""}\n${run.stderr ?? ""}`);
    let report;
    try {
      report = JSON.parse(readFileSync(reportFile, "utf8"));
    } catch {
      return { failed: run.status === 0 ? [] : null, output };
    }
    const notPassed = (report.testResults ?? [])
      .filter((result) => result.status !== "passed")
      .map((result) => String(result.name).replaceAll("\\", "/").toLowerCase());
    const failed = files.filter((rel) => notPassed.some((name) => name.endsWith(rel.toLowerCase())));
    // A non-zero exit that names no file (a setup error) fails every file.
    return { failed: run.status !== 0 && failed.length === 0 ? [...files] : failed, output };
  } finally {
    rmSync(reportDir, { force: true, recursive: true });
  }
}

function runTypecheck(remainingMs, fail, warnings) {
  if (remainingMs < 20_000) {
    warnings.push("type-check skipped: no time left in the grader budget");
    return "type-check SKIPPED (no time left)";
  }
  const run = spawnSync(
    process.execPath,
    [fileURLToPath(import.meta.url), "--typecheck", worktree, repo],
    {
      cwd: worktree,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      timeout: remainingMs,
      windowsHide: true,
    },
  );
  if (run.error?.code === "ETIMEDOUT" || run.signal) {
    warnings.push(`type-check skipped: did not finish within ${Math.round(remainingMs / 1000)}s`);
    return "type-check SKIPPED (timed out)";
  }
  let report;
  try {
    report = JSON.parse(run.stdout);
  } catch {
    warnings.push(`type-check skipped: the compiler run failed:\n${tail(run.stderr || run.stdout)}`);
    return "type-check SKIPPED (compiler run failed)";
  }
  if (report.newErrors.length > 0) {
    fail(
      `${report.newErrors.length} new type error(s) (${report.baseErrors} already present at the base commit are ignored):\n${bullets(report.newErrors, 20)}`,
    );
    return "type-check failed";
  }
  return `type-check clean (${report.currentErrors} errors, none new; ${report.files} files)`;
}

// ---------------------------------------------------------------------------
// Syntax scans
// ---------------------------------------------------------------------------

function scanBase() {
  const listed = git(["grep", "-l", "-w", "-e", OLD_NAME, BASE, "--", "src"], [0, 1]);
  const candidates = listed
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => line.slice(BASE.length + 1))
    .filter(isTypeScript);
  const files = new Map();
  let references = 0;
  let valueImporters = 0;
  let memberUses = 0;
  let mockSites = 0;
  for (const rel of candidates) {
    if (rel === `${OLD_MODULE}.ts`) continue;
    const info = scanFile(rel, git(["show", `${BASE}:${rel}`]));
    const refs = info.literals.filter((literal) => literal.id === OLD_MODULE).length;
    if (refs === 0) continue;
    const locals = info.imports
      .filter((declaration) => declaration.id === OLD_MODULE)
      .flatMap((declaration) => declaration.bindings)
      .filter((binding) => binding.imported === OLD_NAME)
      .map((binding) => binding.local);
    const members = info.members.filter((member) => locals.includes(member.object)).length;
    const mocks = info.mocks.filter((mock) => mock.id === OLD_MODULE).length;
    files.set(rel, { refs, members, mocks });
    references += refs;
    memberUses += members;
    mockSites += mocks;
    if (locals.length > 0) valueImporters += 1;
  }
  const module = scanModule(`${OLD_MODULE}.ts`, git(["show", `${BASE}:${OLD_MODULE}.ts`]), OLD_NAME);
  return { files, references, valueImporters, memberUses, mockSites, module };
}

function scanFile(rel, text) {
  const source = parse(rel, text);
  const info = {
    literals: [],
    imports: [],
    reexports: [],
    members: [],
    mocks: [],
    oldIdentifiers: [],
  };
  const visit = (node) => {
    if (ts.isStringLiteralLike(node)) {
      const id = resolveSpecifier(rel, node.text);
      if (id) info.literals.push({ id, text: node.text, line: lineOf(source, node) });
    } else if (ts.isIdentifier(node) && node.text === OLD_NAME) {
      info.oldIdentifiers.push(lineOf(source, node));
    }
    if (ts.isImportDeclaration(node) && ts.isStringLiteralLike(node.moduleSpecifier)) {
      const bindings = [];
      const clause = node.importClause;
      if (clause?.name) bindings.push({ imported: "default", local: clause.name.text });
      if (clause?.namedBindings) {
        if (ts.isNamespaceImport(clause.namedBindings)) {
          bindings.push({ imported: "*", local: clause.namedBindings.name.text });
        } else {
          for (const element of clause.namedBindings.elements) {
            bindings.push({
              imported: (element.propertyName ?? element.name).text,
              local: element.name.text,
            });
          }
        }
      }
      info.imports.push({
        id: resolveSpecifier(rel, node.moduleSpecifier.text),
        bindings,
        line: lineOf(source, node),
      });
    } else if (
      ts.isExportDeclaration(node) &&
      node.moduleSpecifier &&
      ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      info.reexports.push({
        id: resolveSpecifier(rel, node.moduleSpecifier.text),
        line: lineOf(source, node),
      });
    } else if (ts.isPropertyAccessExpression(node) && ts.isIdentifier(node.expression)) {
      info.members.push({
        object: node.expression.text,
        member: node.name.text,
        line: lineOf(source, node.name),
      });
    } else if (ts.isCallExpression(node) && isMockCall(node)) {
      const keys = new Set();
      const factory = node.arguments[1];
      if (factory) collectPropertyNames(factory, keys);
      info.mocks.push({
        id: resolveSpecifier(rel, node.arguments[0].text),
        hasFactory: Boolean(factory),
        keys,
        line: lineOf(source, node),
      });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return info;
}

function scanModule(rel, text, objectName) {
  const source = parse(rel, text);
  const exports = new Set();
  const declared = new Map();
  const commands = new Set();
  const oldIdentifiers = [];
  let reexports = false;
  for (const statement of source.statements) {
    const exported =
      ts.canHaveModifiers(statement) &&
      (ts.getModifiers(statement) ?? []).some(
        (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
      );
    if (ts.isVariableStatement(statement)) {
      for (const declaration of statement.declarationList.declarations) {
        if (!ts.isIdentifier(declaration.name)) continue;
        declared.set(declaration.name.text, declaration.initializer);
        if (exported) exports.add(declaration.name.text);
      }
    } else if (
      (ts.isTypeAliasDeclaration(statement) ||
        ts.isInterfaceDeclaration(statement) ||
        ts.isFunctionDeclaration(statement) ||
        ts.isClassDeclaration(statement) ||
        ts.isEnumDeclaration(statement)) &&
      exported &&
      statement.name
    ) {
      exports.add(statement.name.text);
    } else if (ts.isExportDeclaration(statement)) {
      if (statement.moduleSpecifier) reexports = true;
      if (statement.exportClause && ts.isNamedExports(statement.exportClause)) {
        for (const element of statement.exportClause.elements) exports.add(element.name.text);
      }
    }
  }
  const visit = (node) => {
    if (ts.isIdentifier(node) && node.text === OLD_NAME && objectName !== OLD_NAME) {
      oldIdentifiers.push(lineOf(source, node));
    }
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === "invoke" &&
      node.arguments.length > 0 &&
      ts.isStringLiteralLike(node.arguments[0])
    ) {
      commands.add(node.arguments[0].text);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);

  let initializer = declared.get(objectName);
  while (
    initializer &&
    (ts.isAsExpression(initializer) ||
      ts.isSatisfiesExpression(initializer) ||
      ts.isParenthesizedExpression(initializer))
  ) {
    initializer = initializer.expression;
  }
  const object = initializer && ts.isObjectLiteralExpression(initializer) ? initializer : null;
  const keys = new Map();
  if (object) {
    for (const property of object.properties) {
      if (!property.name || !(ts.isIdentifier(property.name) || ts.isStringLiteralLike(property.name))) {
        continue;
      }
      let callable = null;
      if (ts.isMethodDeclaration(property)) callable = property;
      else if (
        ts.isPropertyAssignment(property) &&
        (ts.isArrowFunction(property.initializer) || ts.isFunctionExpression(property.initializer))
      ) {
        callable = property.initializer;
      }
      const signature = callable
        ? callable.parameters.map((parameter) => parameter.getText(source).replace(/\s+/g, "")).join(",")
        : "<value>";
      keys.set(property.name.text, signature);
    }
  }
  return { exports, object: Boolean(object), keys, commands, reexports, oldIdentifiers };
}

function parse(rel, text) {
  return ts.createSourceFile(
    rel,
    text,
    ts.ScriptTarget.Latest,
    true,
    rel.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
}

function lineOf(source, node) {
  return source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
}

function isMockCall(node) {
  const callee = node.expression;
  return (
    ts.isPropertyAccessExpression(callee) &&
    ts.isIdentifier(callee.expression) &&
    ["vi", "vitest", "jest"].includes(callee.expression.text) &&
    MOCK_METHODS.has(callee.name.text) &&
    node.arguments.length > 0 &&
    ts.isStringLiteralLike(node.arguments[0])
  );
}

function collectPropertyNames(node, keys) {
  const visit = (child) => {
    if (
      (ts.isPropertyAssignment(child) ||
        ts.isShorthandPropertyAssignment(child) ||
        ts.isMethodDeclaration(child) ||
        ts.isGetAccessorDeclaration(child)) &&
      child.name &&
      (ts.isIdentifier(child.name) || ts.isStringLiteralLike(child.name))
    ) {
      keys.add(child.name.text);
    }
    ts.forEachChild(child, visit);
  };
  visit(node);
}

/** Module id (path from the repository root, no extension) of a relative or
 * `@/` specifier written in `rel`; null for package specifiers. */
function resolveSpecifier(rel, specifier) {
  let target;
  if (specifier.startsWith("@/")) target = posix.join("src", specifier.slice(2));
  else if (/^\.\.?(\/|$)/.test(specifier)) target = posix.join(posix.dirname(rel), specifier);
  else return null;
  return posix
    .normalize(target)
    .replace(/\.(d\.ts|tsx?|jsx?|mjs|cjs)$/, "")
    .replace(/\/index$/, "");
}

function isTypeScript(rel) {
  return /\.tsx?$/.test(rel);
}

function listSourceFiles() {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(join(worktree, dir), { withFileTypes: true })) {
      const rel = `${dir}/${entry.name}`;
      if (entry.isSymbolicLink() || entry.name === "node_modules") continue;
      if (entry.isDirectory()) walk(rel);
      else if (entry.isFile() && isTypeScript(rel)) out.push(rel);
    }
  };
  if (existsSync(join(worktree, "src"))) walk("src");
  return out;
}

// ---------------------------------------------------------------------------
// Type-check (child process: `--typecheck <worktree> <repo>`)
// ---------------------------------------------------------------------------

function typecheckMain() {
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
    const line = diagnostic.file
      ? diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start ?? 0).line + 1
      : 0;
    return { id: `${file}|TS${diagnostic.code}|${message}`, text: `${file}:${line} TS${diagnostic.code} ${message}` };
  };
  const errorsOf = (program, extra = []) =>
    [...extra, ...ts.getPreEmitDiagnostics(program)]
      .filter((diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error)
      .map(describe);

  const program = ts.createProgram({ rootNames: parsed.fileNames, options });
  const current = errorsOf(program, [...(config.error ? [config.error] : []), ...parsed.errors]);
  let baseIds = new Set();
  if (current.length > 0) {
    // Rebuild the program as the base commit saw it: every path that differs
    // from the base commit is served from git instead of from disk.
    const overlay = new Map();
    const added = new Set();
    const restored = [];
    for (const line of git(["diff", "--name-status", "--no-renames", BASE, "--", "."]).split(/\r?\n/)) {
      const [status, path] = line.split("\t");
      if (!path) continue;
      const absolute = key(join(worktree, path));
      if (status === "A") {
        overlay.set(absolute, null);
        added.add(absolute);
      } else {
        overlay.set(absolute, git(["show", `${BASE}:${path}`]));
        if (status === "D" && path.startsWith("src/") && isTypeScript(path)) {
          restored.push(join(worktree, path).replace(/\\/g, "/"));
        }
      }
    }
    for (const path of git(["ls-files", "--others", "--exclude-standard"]).split(/\r?\n/)) {
      if (!path) continue;
      overlay.set(key(join(worktree, path)), null);
      added.add(key(join(worktree, path)));
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
    baseIds = new Set(errorsOf(ts.createProgram({ rootNames, options, host })).map((error) => error.id));
  }
  const newErrors = current.filter((error) => !baseIds.has(error.id)).map((error) => error.text);
  process.stdout.write(
    JSON.stringify({
      files: parsed.fileNames.length,
      currentErrors: current.length,
      baseErrors: baseIds.size,
      newErrors,
    }),
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function changedPaths() {
  const tracked = git(["diff", "--name-only", "--no-renames", BASE, "--", "."]);
  const untracked = git(["ls-files", "--others", "--exclude-standard"]);
  return `${tracked}\n${untracked}`.split(/\r?\n/).filter(Boolean);
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

function bullets(items, limit = 15) {
  const shown = items.slice(0, limit).map((item) => `   - ${item}`);
  if (items.length > limit) shown.push(`   - ... and ${items.length - limit} more`);
  return shown.join("\n");
}

function tail(value) {
  return String(value ?? "").slice(-4000);
}
