#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

const worktree = resolve(process.argv[2] ?? ".");
const packagePath = join(
  "fr",
  "goldeneyetools",
  "abyssalzenith",
  "security",
);
const mainDir = join(worktree, "src", "main", "java", packagePath);
const testDir = join(worktree, "src", "test", "java", packagePath);
const utilityPath = join(mainDir, "PublicEndpointPaths.java");
const securityConfigPath = join(mainDir, "SecurityConfig.java");
const filterPath = join(mainDir, "JwtAuthenticationFilter.java");
const heldOutPath = join(testDir, "AgentBenchPublicEndpointPathsTest.java");
const failures = [];

requireFile(utilityPath, "PublicEndpointPaths.java was not added");
requireFile(securityConfigPath, "SecurityConfig.java is missing");
requireFile(filterPath, "JwtAuthenticationFilter.java is missing");

if (existsSync(securityConfigPath)) {
  requireMatch(
    readFileSync(securityConfigPath, "utf8"),
    /PublicEndpointPaths\s*\.\s*patterns\s*\(/,
    "SecurityConfig does not use PublicEndpointPaths.patterns()",
  );
}
if (existsSync(filterPath)) {
  requireMatch(
    readFileSync(filterPath, "utf8"),
    /PublicEndpointPaths\s*\.\s*matches\s*\(/,
    "JwtAuthenticationFilter does not delegate to PublicEndpointPaths.matches()",
  );
}

const changedTests = [
  ...git(["diff", "--name-only", "--", "src/test/java"]).split(/\r?\n/),
  ...git(["ls-files", "--others", "--exclude-standard", "--", "src/test/java"]).split(/\r?\n/),
].filter(Boolean);
if (changedTests.length === 0) failures.push("focused Java tests were not added");

const heldOut = `package fr.goldeneyetools.abyssalzenith.security;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Arrays;
import org.junit.jupiter.api.Test;

class AgentBenchPublicEndpointPathsTest {

    private static final String[] EXPECTED = {
        "/api/auth/**",
        "/swagger-ui.html",
        "/swagger-ui/**",
        "/v3/api-docs/**",
        "/v3/api-docs.yaml",
        "/error",
        "/api/weekly-rotations/active"
    };

    @Test
    void exposesExactlyTheSharedSpringPatterns() {
        String[] actual = PublicEndpointPaths.patterns();
        Arrays.sort(actual);
        String[] expected = EXPECTED.clone();
        Arrays.sort(expected);
        assertArrayEquals(expected, actual);
    }

    @Test
    void returnsADefensivePatternCopy() {
        String[] first = PublicEndpointPaths.patterns();
        first[0] = "/mutated";
        assertFalse(Arrays.asList(PublicEndpointPaths.patterns()).contains("/mutated"));
    }

    @Test
    void matchesExactAndWildcardBasePaths() {
        assertTrue(PublicEndpointPaths.matches("/api/auth"));
        assertTrue(PublicEndpointPaths.matches("/api/auth/login"));
        assertTrue(PublicEndpointPaths.matches("/swagger-ui"));
        assertTrue(PublicEndpointPaths.matches("/swagger-ui/index.html"));
        assertTrue(PublicEndpointPaths.matches("/swagger-ui.html"));
        assertTrue(PublicEndpointPaths.matches("/v3/api-docs"));
        assertTrue(PublicEndpointPaths.matches("/v3/api-docs/users"));
        assertTrue(PublicEndpointPaths.matches("/v3/api-docs.yaml"));
        assertTrue(PublicEndpointPaths.matches("/error"));
        assertTrue(PublicEndpointPaths.matches("/api/weekly-rotations/active"));
    }

    @Test
    void rejectsNullDescendantsOfExactPathsAndLookalikePrefixes() {
        assertFalse(PublicEndpointPaths.matches(null));
        assertFalse(PublicEndpointPaths.matches("/api/authentication"));
        assertFalse(PublicEndpointPaths.matches("/swagger-ui-custom"));
        assertFalse(PublicEndpointPaths.matches("/v3/api-docs.yaml/extra"));
        assertFalse(PublicEndpointPaths.matches("/error/details"));
        assertFalse(PublicEndpointPaths.matches("/api/weekly-rotations/active/extra"));
    }
}
`;

let original = null;
try {
  mkdirSync(testDir, { recursive: true });
  if (existsSync(heldOutPath)) original = readFileSync(heldOutPath, "utf8");
  writeFileSync(heldOutPath, heldOut);
  const command = process.platform === "win32" ? "cmd.exe" : "mvn";
  const args = process.platform === "win32"
    ? ["/d", "/s", "/c", "mvn -q -Dtest=AgentBenchPublicEndpointPathsTest test"]
    : ["-q", "-Dtest=AgentBenchPublicEndpointPathsTest", "test"];
  const run = spawnSync(
    command,
    args,
    {
      cwd: worktree,
      encoding: "utf8",
      timeout: 240_000,
      windowsHide: true,
    },
  );
  if (run.error) failures.push(`held-out Maven test failed to start: ${run.error.message}`);
  else if (run.status !== 0) failures.push(`held-out Maven tests failed:\n${tail(run.stderr || run.stdout)}`);
} finally {
  if (original === null) rmSync(heldOutPath, { force: true });
  else writeFileSync(heldOutPath, original);
}

const diffCheck = spawnSync("git", ["-C", worktree, "diff", "--check"], {
  encoding: "utf8",
  windowsHide: true,
});
if (diffCheck.status !== 0) {
  failures.push(`git diff --check failed:\n${tail(diffCheck.stderr || diffCheck.stdout)}`);
}

if (failures.length > 0) {
  console.error(failures.join("\n\n"));
  process.exitCode = 1;
} else {
  console.log("PASS: shared public-endpoint policy, boundary matching, wiring, and tests verified");
}

function git(args) {
  const result = spawnSync("git", ["-C", worktree, ...args], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0) failures.push(`git ${args.join(" ")} failed: ${tail(result.stderr)}`);
  return result.stdout ?? "";
}

function requireFile(path, message) {
  if (!existsSync(path)) failures.push(message);
}

function requireMatch(source, pattern, message) {
  if (!pattern.test(source)) failures.push(message);
}

function tail(value) {
  return String(value ?? "").slice(-4000);
}
