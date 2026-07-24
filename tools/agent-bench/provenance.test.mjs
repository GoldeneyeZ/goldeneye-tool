import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  captureRepositoryProvenance,
  compareProvenance,
  selectDependencyLock,
} from "./provenance.mjs";

function git(cwd, args) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

async function fixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), "agent-bench-provenance-"));
  const repo = path.join(root, "repo");
  await mkdir(path.join(repo, "dist"), { recursive: true });
  await mkdir(path.join(repo, "target", "release"), { recursive: true });
  await writeFile(path.join(repo, "tracked.txt"), "base\n");
  await writeFile(path.join(repo, "dist", "main.js"), "console.log('base');\n");
  await writeFile(path.join(repo, "package-lock.json"), "{\"lockfileVersion\":3}\n");
  await writeFile(path.join(repo, "target", "release", "goldeneye.exe"), "binary-v1");
  git(repo, ["init"]);
  git(repo, ["config", "user.email", "bench@example.test"]);
  git(repo, ["config", "user.name", "Benchmark"]);
  git(repo, ["add", "."]);
  git(repo, ["commit", "-m", "fixture"]);
  await writeFile(path.join(repo, "tracked.txt"), "modified\n");
  await writeFile(path.join(repo, "z-untracked.txt"), "z\n");
  await writeFile(path.join(repo, "a-untracked.txt"), "a\n");
  t.after(() => rm(root, { recursive: true, force: true }));
  return repo;
}

test("captureRepositoryProvenance is raw-byte deterministic and sorted", async (t) => {
  const repo = await fixture(t);
  const options = {
    repo,
    selectedFiles: ["target/release/goldeneye.exe", "dist/main.js"],
  };
  const first = captureRepositoryProvenance(options);
  const second = captureRepositoryProvenance(options);

  assert.deepEqual(first, second);
  assert.match(first.repo_head, /^[0-9a-f]{40}$/);
  assert.match(first.tracked_diff_sha256, /^[0-9a-f]{64}$/);
  assert.deepEqual(first.untracked.map(({ path: entry }) => entry), ["a-untracked.txt", "z-untracked.txt"]);
  assert.deepEqual(first.selected_files.map(({ path: entry }) => entry), ["dist/main.js", "target/release/goldeneye.exe"]);
  for (const entry of [...first.untracked, ...first.selected_files]) {
    assert.ok(entry.bytes > 0);
    assert.match(entry.sha256, /^[0-9a-f]{64}$/);
    assert.notEqual(entry.sha256, "0".repeat(64));
  }
});

test("captureRepositoryProvenance detects tracked, untracked, and selected-byte changes", async (t) => {
  const repo = await fixture(t);
  const options = { repo, selectedFiles: ["target/release/goldeneye.exe"] };
  const baseline = captureRepositoryProvenance(options);

  await writeFile(path.join(repo, "tracked.txt"), "modified-again\n");
  assert.equal(compareProvenance(baseline, captureRepositoryProvenance(options)).field, "tracked_diff_sha256");

  await writeFile(path.join(repo, "tracked.txt"), "modified\n");
  await writeFile(path.join(repo, "a-untracked.txt"), "b\n");
  assert.equal(compareProvenance(baseline, captureRepositoryProvenance(options)).field, "untracked[0].sha256");

  await writeFile(path.join(repo, "a-untracked.txt"), "a\n");
  await writeFile(path.join(repo, "target", "release", "goldeneye.exe"), "binary-v2");
  assert.equal(compareProvenance(baseline, captureRepositoryProvenance(options)).field, "selected_files[0].sha256");
});

test("compareProvenance reports the first exact mismatched field", async (t) => {
  const repo = await fixture(t);
  const baseline = captureRepositoryProvenance({ repo, selectedFiles: ["dist/main.js"] });
  const changed = structuredClone(baseline);
  changed.repo_head = "f".repeat(40);
  assert.deepEqual(compareProvenance(baseline, changed), {
    equal: false,
    field: "repo_head",
    expected: baseline.repo_head,
    observed: changed.repo_head,
  });
});

test("Goldeneye candidate provenance rejects an unrelated tracked diff change", async (t) => {
  const repo = await fixture(t);
  const baseline = captureRepositoryProvenance({
    repo,
    selectedFiles: ["dist/main.js", "target/release/goldeneye.exe"],
  });
  await writeFile(path.join(repo, "tracked.txt"), "different-candidate-change\n");
  const observed = captureRepositoryProvenance({
    repo,
    selectedFiles: ["dist/main.js", "target/release/goldeneye.exe"],
  });
  assert.equal(compareProvenance(baseline, observed).field, "tracked_diff_sha256");
});

test("ACK provenance selects deterministic dependency lock and rejects lock mutation", async (t) => {
  const repo = await fixture(t);
  assert.equal(selectDependencyLock(repo), "package-lock.json");
  const selectedFiles = ["dist/main.js", selectDependencyLock(repo)];
  const baseline = captureRepositoryProvenance({ repo, selectedFiles });
  await writeFile(path.join(repo, "package-lock.json"), "{\"lockfileVersion\":4}\n");
  const observed = captureRepositoryProvenance({ repo, selectedFiles });
  const lockIndex = baseline.selected_files.findIndex((entry) => entry.path === "package-lock.json");
  assert.equal(compareProvenance(baseline, observed).field, `selected_files[${lockIndex}].sha256`);
});

test("selectDependencyLock fails closed without a supported lockfile", async (t) => {
  const repo = await fixture(t);
  await rm(path.join(repo, "package-lock.json"));
  assert.throws(() => selectDependencyLock(repo), /dependency lockfile/);
});
