import assert from "node:assert/strict";
import { access, readFile, readdir } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const BENCH_ROOT = fileURLToPath(new URL(".", import.meta.url));
const REPO_ROOT = path.resolve(BENCH_ROOT, "../..");
const BIN_ROOT = path.join(BENCH_ROOT, "bin");

const NEW_ENTRYPOINTS = [
  path.join(BIN_ROOT, "benchmark-agent-tasks.mjs"),
  path.join(BIN_ROOT, "benchmark-competitors.mjs"),
];

const OLD_ENTRYPOINTS = [
  path.join(REPO_ROOT, "tools", "benchmark-agent-tasks.mjs"),
  path.join(REPO_ROOT, "tools", "benchmark-competitors.mjs"),
];

async function exists(candidate) {
  try {
    await access(candidate);
    return true;
  }
  catch {
    return false;
  }
}

async function collectFiles(root) {
  const entries = await readdir(root, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const candidate = path.join(root, entry.name);
    return entry.isDirectory() ? collectFiles(candidate) : [candidate];
  }));
  return nested.flat();
}

test("benchmark runtime entrypoints live only under tools/agent-bench", async () => {
  for (const entrypoint of NEW_ENTRYPOINTS) {
    assert.equal(await exists(entrypoint), true, `missing ${entrypoint}`);
  }
  for (const entrypoint of OLD_ENTRYPOINTS) {
    assert.equal(await exists(entrypoint), false, `legacy entrypoint remains: ${entrypoint}`);
  }
});

test("entrypoint relative imports remain inside tools/agent-bench", async () => {
  for (const entrypoint of NEW_ENTRYPOINTS) {
    const source = await readFile(entrypoint, "utf8");
    const imports = [...source.matchAll(
      /(?:from\s*|import\s*\()\s*["']([^"']+)["']/g,
    )].map((match) => match[1]);

    for (const specifier of imports.filter((value) => value.startsWith("."))) {
      const resolved = path.resolve(path.dirname(entrypoint), specifier);
      assert.equal(
        path.relative(BENCH_ROOT, resolved).startsWith(".."),
        false,
        `${entrypoint} escapes bench root through ${specifier}`,
      );
    }
  }
});

test("production Rust sources do not reference benchmark runtime paths", async () => {
  const candidates = [
    path.join(REPO_ROOT, "Cargo.toml"),
    ...(await collectFiles(path.join(REPO_ROOT, "crates")))
      .filter((candidate) => /\.(?:rs|toml)$/.test(candidate)),
  ];
  const forbidden = /tools[\\/]agent-bench|benchmark-agent-tasks|benchmark-competitors/;

  for (const candidate of candidates) {
    const source = await readFile(candidate, "utf8");
    assert.doesNotMatch(source, forbidden, candidate);
  }
});
