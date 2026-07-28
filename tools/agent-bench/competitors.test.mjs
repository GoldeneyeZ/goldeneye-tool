import assert from "node:assert/strict";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const BENCH_ROOT = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = realpathSync(join(BENCH_ROOT, "../.."));
const COMPETITOR_RUNNER = join(BENCH_ROOT, "bin", "benchmark-competitors.mjs");

test("competitor runner derives repository-root defaults after relocation", () => {
  const source = readFileSync(COMPETITOR_RUNNER, "utf8");
  const workspaceParent = source.match(
    /const workspace = resolve\(dirname\(fileURLToPath\(import\.meta\.url\)\), "([^"]+)"\);/,
  )?.[1];

  assert.ok(workspaceParent, "competitor workspace derivation is missing");
  const workspace = realpathSync(resolve(dirname(COMPETITOR_RUNNER), workspaceParent));

  assert.equal(workspace, REPO_ROOT);
  assert.equal(
    join(workspace, "target", "release", "goldeneye"),
    join(REPO_ROOT, "target", "release", "goldeneye"),
  );
  assert.equal(
    join(workspace, "target", "benchmarks", "latest.json"),
    join(REPO_ROOT, "target", "benchmarks", "latest.json"),
  );
  assert.match(
    source,
    /spawnSync\("cargo", \["build", "--release", "-p", "goldeneye"\], \{\s*cwd: workspace,/,
  );
});
