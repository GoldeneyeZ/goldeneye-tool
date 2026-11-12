#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";

const worktree = resolve(process.argv[2] ?? ".");
const oracle = resolve(
  process.argv[3] ??
    "/home/goldeneye/IdeaProjects/.gab-jdk-grader/app-resources-oracle",
);
const target = "cf0239364b06694c6f61b6ea2b04c2f244954df8";
const failures = [];

const corePaths = [
  "src/jdk.jpackage/linux/classes/jdk/jpackage/internal/LinuxPackageBuilder.java",
  "src/jdk.jpackage/linux/classes/jdk/jpackage/internal/LinuxPackagingPipeline.java",
  "src/jdk.jpackage/macosx/classes/jdk/jpackage/internal/MacPackagingPipeline.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/ApplicationBuilder.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/ApplicationImageUtils.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/FromOptions.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/cli/StandardHelpFormatter.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/cli/StandardOption.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/Application.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/ApplicationLayout.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/ApplicationLayoutMixin.java",
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/resources/HelpResources.properties",
  "src/jdk.jpackage/share/man/jpackage.md",
];

for (const path of corePaths) {
  if (!existsSync(join(worktree, path))) failures.push(`missing core file: ${path}`);
}

check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/cli/StandardOption.java",
  /APP_RESOURCES\s*=\s*existingPathOption\("app-resources"\)[\s\S]*?\.tokenizer\(pathSeparator\(\)\)[\s\S]*?\.outOfScope\(NOT_BUILDING_APP_IMAGE\)[\s\S]*?\.createArray/,
  "APP_RESOURCES must be repeatable, path-separator aware, and app-image scoped",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/FromOptions.java",
  /APP_RESOURCES\s*\.\s*findIn[\s\S]*?reversed\(\)[\s\S]*?resourcesDirSources/,
  "FromOptions must preserve repeated-option precedence and populate resources",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/ApplicationBuilder.java",
  /resourcesDirSources[\s\S]*?Application\s*\(/,
  "ApplicationBuilder must carry resource sources into Application",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/Application.java",
  /Collection<RootedPath>\s+resourcesDirSources/,
  "Application model lacks immutable resource sources",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/ApplicationLayout.java",
  /resourcesDirectory[\s\S]*?Objects\.requireNonNull\(resourcesDirectory\)/,
  "ApplicationLayout builder must require a resources directory",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/model/ApplicationLayoutMixin.java",
  /Path\s+resourcesDirectory\s*\(\)/,
  "ApplicationLayoutMixin lacks resourcesDirectory()",
);
check(
  "src/jdk.jpackage/linux/classes/jdk/jpackage/internal/LinuxPackagingPipeline.java",
  /resourcesDirectory\("lib"\)/,
  "Linux app resources must map to lib",
);
check(
  "src/jdk.jpackage/macosx/classes/jdk/jpackage/internal/MacPackagingPipeline.java",
  /resourcesDirectory\("Contents\/Resources"\)/,
  "macOS app resources must map to Contents/Resources",
);
check(
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/resources/HelpResources.properties",
  /help\.option\.app-resources/,
  "localized help lacks --app-resources",
);
check(
  "src/jdk.jpackage/share/man/jpackage.md",
  /--app-resources[\s\S]*?platform-specific path separator[\s\S]*?--app-content/,
  "manual lacks separator/destination/precedence documentation",
);

const imageUtilsPath = join(
  worktree,
  "src/jdk.jpackage/share/classes/jdk/jpackage/internal/ApplicationImageUtils.java",
);
if (existsSync(imageUtilsPath)) {
  const source = readFileSync(imageUtilsPath, "utf8");
  const input = source.indexOf("appDirSources()");
  const resources = source.indexOf("resourcesDirSources()");
  const content = source.indexOf("contentDirSources()");
  if (!(input >= 0 && input < resources && resources < content)) {
    failures.push("copy order must be input, app resources, then app content");
  }
}

const changed = git(["diff", "--name-only", "HEAD"]);
const changedTests = changed
  .split(/\r?\n/)
  .filter((path) => path.startsWith("test/jdk/tools/jpackage/") && path.endsWith(".java"));
if (changedTests.length < 4) {
  failures.push(`expected at least 4 focused changed Java tests; found ${changedTests.length}`);
}

if (!existsSync(join(oracle, "build", "linux-x86_64-server-release", "spec.gmk"))) {
  failures.push("configured OpenJDK oracle workspace is unavailable");
} else if (failures.length === 0) {
  runOracleChecks();
}

const diffCheck = run("git", ["-C", worktree, "diff", "--check"], worktree, 30_000);
if (diffCheck.status !== 0) failures.push(`git diff --check failed:\n${tail(diffCheck)}`);

if (failures.length) {
  console.error(failures.join("\n\n"));
  process.exitCode = 1;
} else {
  console.log(
    "PASS: jpackage app-resources CLI, model/layout wiring, copy precedence, build, smoke test, docs, and tests verified",
  );
}

function runOracleChecks() {
  const temp = mkdtempSync(join(tmpdir(), "jdk-app-resources-grade-"));
  try {
    const reset = run("git", ["reset", "--hard", target], oracle, 60_000);
    if (reset.status !== 0) {
      failures.push(`oracle reset failed:\n${tail(reset)}`);
      return;
    }

    for (const path of corePaths.filter((path) => !path.endsWith(".md"))) {
      const source = join(worktree, path);
      const destination = join(oracle, path);
      mkdirSync(dirname(destination), { recursive: true });
      copyFileSync(source, destination);
    }

    const build = run("make", ["jdk.jpackage"], oracle, 900_000);
    if (build.status !== 0) {
      failures.push(`jdk.jpackage build failed:\n${tail(build)}`);
      return;
    }

    const buildRoot = join(oracle, "build", "linux-x86_64-server-release", "jdk");
    const jpackage = join(buildRoot, "bin", "jpackage");
    const javac = "/home/goldeneye/.sdkman/candidates/java/26.0.2+1.1-tem/bin/javac";
    const jar = "/home/goldeneye/.sdkman/candidates/java/26.0.2+1.1-tem/bin/jar";
    if (!existsSync(jpackage)) {
      failures.push("built jpackage executable is missing");
      return;
    }

    const classes = join(temp, "classes");
    const input = join(temp, "input");
    const resourcesA = join(temp, "resources-a");
    const resourcesB = join(temp, "resources-b");
    const resourcesC = join(temp, "resources-c");
    const content = join(temp, "content");
    const output = join(temp, "output");
    for (const dir of [classes, input, resourcesA, resourcesB, resourcesC, content, output]) {
      mkdirSync(dir, { recursive: true });
    }
    mkdirSync(join(resourcesA, "a", "b"), { recursive: true });
    writeFileSync(join(temp, "Hello.java"), "public class Hello { public static void main(String[] a) {} }\n");
    writeFileSync(join(resourcesA, "a", "b", "c.txt"), "nested\n");
    writeFileSync(join(resourcesB, "second.txt"), "second\n");
    writeFileSync(join(resourcesB, "collision.txt"), "resource\n");
    writeFileSync(join(resourcesC, "third.txt"), "third\n");
    writeFileSync(join(content, "collision.txt"), "content\n");

    const compile = run(javac, ["-d", classes, join(temp, "Hello.java")], temp, 60_000);
    if (compile.status !== 0) {
      failures.push(`smoke app compile failed:\n${tail(compile)}`);
      return;
    }
    const archive = run(
      jar,
      ["--create", "--file", join(input, "app.jar"), "--main-class", "Hello", "-C", classes, "."],
      temp,
      60_000,
    );
    if (archive.status !== 0) {
      failures.push(`smoke app archive failed:\n${tail(archive)}`);
      return;
    }

    const packageRun = run(
      jpackage,
      [
        "--type", "app-image",
        "--dest", output,
        "--name", "BenchApp",
        "--input", input,
        "--main-jar", "app.jar",
        "--main-class", "Hello",
        "--runtime-image", "/home/goldeneye/.sdkman/candidates/java/26.0.2+1.1-tem",
        "--app-content", join(content, "collision.txt"),
        "--app-resources", [
          resourcesA,
          join(resourcesB, "second.txt"),
          join(resourcesB, "collision.txt"),
        ].join(":"),
        "--app-resources", resourcesC,
      ],
      temp,
      180_000,
    );
    if (packageRun.status !== 0) {
      failures.push(`jpackage app-resources smoke test failed:\n${tail(packageRun)}`);
      return;
    }

    const lib = join(output, "BenchApp", "lib");
    expectFile(join(lib, "resources-a", "a", "b", "c.txt"), "nested\n");
    expectFile(join(lib, "second.txt"), "second\n");
    expectFile(join(lib, "resources-c", "third.txt"), "third\n");
    expectFile(join(lib, "collision.txt"), "content\n");

    const help = run(jpackage, ["--help"], temp, 60_000);
    if (help.status !== 0 || !`${help.stdout}\n${help.stderr}`.includes("--app-resources")) {
      failures.push("jpackage --help does not expose --app-resources");
    }
  } finally {
    run("git", ["reset", "--hard", target], oracle, 60_000);
    rmSync(temp, { recursive: true, force: true });
  }
}

function check(path, pattern, message) {
  const fullPath = join(worktree, path);
  if (!existsSync(fullPath)) return;
  if (!pattern.test(readFileSync(fullPath, "utf8"))) failures.push(message);
}

function expectFile(path, expected) {
  if (!existsSync(path)) failures.push(`missing packaged resource: ${path}`);
  else if (readFileSync(path, "utf8") !== expected) {
    failures.push(`unexpected packaged resource content: ${path}`);
  }
}

function git(args) {
  const result = run("git", ["-C", worktree, ...args], worktree, 30_000);
  if (result.status !== 0) failures.push(`git ${args.join(" ")} failed:\n${tail(result)}`);
  return result.stdout ?? "";
}

function run(command, args, cwd, timeout) {
  return spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    timeout,
    windowsHide: true,
    env: process.env,
  });
}

function tail(result) {
  if (result.error) return result.error.message;
  return `${result.stderr ?? ""}\n${result.stdout ?? ""}`.slice(-5000);
}
