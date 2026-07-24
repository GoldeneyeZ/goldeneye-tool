import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync } from "node:fs";
import path from "node:path";

const ZERO_SHA256 = "0".repeat(64);

export function captureRepositoryProvenance({ repo, selectedFiles = [] }) {
  const root = path.resolve(repo);
  const repoHead = git(root, ["rev-parse", "HEAD"]).toString("utf8").trim();
  const trackedDiff = git(root, ["diff", "--binary", "--no-ext-diff"]);
  const untracked = git(root, ["ls-files", "--others", "--exclude-standard", "-z"])
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .sort()
    .map((relative) => fileFingerprint(root, relative));
  const selected = [...new Set(selectedFiles)]
    .map((entry) => normalizeRelative(root, entry))
    .sort()
    .map((relative) => fileFingerprint(root, relative));

  const provenance = {
    repo_head: repoHead,
    tracked_diff_sha256: sha256(trackedDiff),
    untracked,
    selected_files: selected,
  };
  assertObservedHashes(provenance);
  return provenance;
}

export function compareProvenance(expected, observed) {
  const difference = firstDifference(expected, observed, "");
  return difference ?? { equal: true, field: null, expected: null, observed: null };
}

export function selectDependencyLock(repo) {
  const root = path.resolve(repo);
  for (const candidate of ["package-lock.json", "pnpm-lock.yaml", "yarn.lock"]) {
    const absolute = path.join(root, candidate);
    if (existsSync(absolute) && lstatSync(absolute).isFile()) return candidate;
  }
  throw new Error(`dependency lockfile missing in ${root}; expected package-lock.json, pnpm-lock.yaml, or yarn.lock`);
}

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function fileFingerprint(root, relative) {
  const absolute = path.resolve(root, relative);
  const details = lstatSync(absolute);
  if (!details.isFile()) throw new Error(`provenance target is not a regular file: ${relative}`);
  const bytes = readFileSync(absolute);
  return { path: toPortable(relative), bytes: details.size, sha256: sha256(bytes) };
}

function normalizeRelative(root, entry) {
  const absolute = path.resolve(root, entry);
  const relative = path.relative(root, absolute);
  if (!relative || path.isAbsolute(relative) || relative === ".." || relative.startsWith(`..${path.sep}`)) {
    throw new Error(`provenance target escapes repository: ${entry}`);
  }
  return relative;
}

function git(cwd, args) {
  const result = spawnSync("git", ["-C", cwd, ...args], {
    encoding: null,
    maxBuffer: 50 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    const detail = result.error?.message ?? result.stderr?.toString("utf8").trim() ?? "unknown git failure";
    throw new Error(`git ${args.join(" ")} failed: ${detail}`);
  }
  return result.stdout;
}

function assertObservedHashes(provenance) {
  const hashes = [
    provenance.tracked_diff_sha256,
    ...provenance.untracked.map((entry) => entry.sha256),
    ...provenance.selected_files.map((entry) => entry.sha256),
  ];
  for (const hash of hashes) {
    if (!/^[a-f0-9]{64}$/.test(hash) || hash === ZERO_SHA256) {
      throw new Error(`invalid observed SHA-256: ${hash}`);
    }
  }
}

function firstDifference(expected, observed, field) {
  if (Object.is(expected, observed)) return null;
  if (Array.isArray(expected) || Array.isArray(observed)) {
    if (!Array.isArray(expected) || !Array.isArray(observed)) return mismatch(field, expected, observed);
    if (expected.length !== observed.length) return mismatch(`${field}.length`, expected.length, observed.length);
    for (let index = 0; index < expected.length; index += 1) {
      const difference = firstDifference(expected[index], observed[index], `${field}[${index}]`);
      if (difference) return difference;
    }
    return null;
  }
  if (isObject(expected) || isObject(observed)) {
    if (!isObject(expected) || !isObject(observed)) return mismatch(field, expected, observed);
    const keys = [...new Set([...Object.keys(expected), ...Object.keys(observed)])].sort();
    for (const key of keys) {
      const nested = field ? `${field}.${key}` : key;
      if (!(key in expected) || !(key in observed)) return mismatch(nested, expected[key], observed[key]);
      const difference = firstDifference(expected[key], observed[key], nested);
      if (difference) return difference;
    }
    return null;
  }
  return mismatch(field, expected, observed);
}

function isObject(value) {
  return value !== null && typeof value === "object";
}

function mismatch(field, expected, observed) {
  return { equal: false, field, expected, observed };
}

function toPortable(value) {
  return value.split(path.sep).join("/");
}
