import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { copyFile, lstat, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const MANIFEST_FILE = "snapshot-manifest.json";
const WRITER_ARTIFACT = /(?:^|[\\/])(?:goldeneye\.db-(?:wal|shm)|.*\.(?:lock|lck))$/i;

export function assertContainedPath(candidate, allowedRoot, label) {
  const root = path.resolve(allowedRoot);
  const target = path.resolve(candidate);
  const relative = path.relative(root, target);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`${label} must be a strict descendant of ${root}: ${target}`);
  }
  return target;
}

function assertDisjointPaths(first, second) {
  const relative = path.relative(first, second);
  const inverse = path.relative(second, first);
  if (!relative || !inverse || (!relative.startsWith("..") && !path.isAbsolute(relative)) ||
      (!inverse.startsWith("..") && !path.isAbsolute(inverse))) {
    throw new Error(`snapshot paths must not overlap: ${first} and ${second}`);
  }
}

function unsafeEntryError(absolute) {
  return new Error(`unsafe filesystem entry: ${absolute}`);
}

export async function collectRegularFiles(root) {
  const resolvedRoot = path.resolve(root);
  const rootStats = await lstat(resolvedRoot);
  if (rootStats.isSymbolicLink() || !rootStats.isDirectory()) {
    throw unsafeEntryError(resolvedRoot);
  }
  const files = [];

  async function walk(directory, relativeDirectory) {
    const entries = await readdir(directory);
    entries.sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
    for (const name of entries) {
      const absolute = path.join(directory, name);
      const relative = relativeDirectory ? `${relativeDirectory}/${name}` : name;
      const stats = await lstat(absolute);
      if (stats.isSymbolicLink()) {
        throw unsafeEntryError(absolute);
      }
      if (stats.isDirectory()) {
        await walk(absolute, relative);
      }
      else if (stats.isFile()) {
        files.push({ absolute, relative });
      }
      else {
        throw unsafeEntryError(absolute);
      }
    }
  }

  await walk(resolvedRoot, "");
  return files;
}

export async function assertNoWriterArtifacts(root) {
  const entries = await collectRegularFiles(root);
  const hit = entries.find(({ absolute }) => WRITER_ARTIFACT.test(absolute));
  if (hit) {
    throw new Error(`snapshot source is not quiescent: ${hit.absolute}`);
  }
}

export function checkpointSqliteDatabase(databasePath) {
  const target = path.resolve(databasePath);
  if (!existsSync(target)) {
    throw new Error(`SQLite database does not exist: ${target}`);
  }
  const database = new DatabaseSync(target);
  let closed = false;
  let journalMode;
  let checkpoint;
  try {
    journalMode = database.prepare("PRAGMA journal_mode").get().journal_mode;
    [checkpoint] = database.prepare("PRAGMA wal_checkpoint(TRUNCATE)").all();
    if (!checkpoint || checkpoint.busy !== 0) {
      throw new Error(`SQLite checkpoint remained busy: ${JSON.stringify(checkpoint ?? null)}`);
    }
    database.close();
    closed = true;
  }
  finally {
    if (!closed) database.close();
  }
  const sidecars = [`${target}-wal`, `${target}-shm`].filter((candidate) => existsSync(candidate));
  if (sidecars.length > 0) {
    throw new Error(`SQLite checkpoint left writer artifacts: ${sidecars.join(", ")}`);
  }
  return {
    database: target,
    journal_mode: journalMode,
    busy: checkpoint.busy,
    log: checkpoint.log,
    checkpointed: checkpoint.checkpointed,
    closed: true,
    sidecars_absent: true,
  };
}

export async function copyRegularTree(source, destination, { exclude = [] } = {}) {
  const excluded = new Set(exclude);
  const entries = await collectRegularFiles(source);
  for (const { absolute, relative } of entries) {
    if (excluded.has(relative)) {
      continue;
    }
    const target = path.join(destination, ...relative.split("/"));
    await mkdir(path.dirname(target), { recursive: true });
    await copyFile(absolute, target);
  }
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

export async function buildManifest(root, { projectRoot, baseRef }) {
  const entries = await collectRegularFiles(root);
  const files = [];
  let byteCount = 0;
  for (const entry of entries) {
    if (entry.relative === MANIFEST_FILE) {
      continue;
    }
    const bytes = await readFile(entry.absolute);
    byteCount += bytes.length;
    files.push({ path: entry.relative, bytes: bytes.length, sha256: sha256(bytes) });
  }
  return {
    schema_version: 1,
    project_root: path.resolve(projectRoot),
    base_ref: baseRef,
    file_count: files.length,
    byte_count: byteCount,
    files,
  };
}

export async function verifyTreeAgainstManifest(root, manifest) {
  const actual = await buildManifest(root, {
    projectRoot: manifest.project_root,
    baseRef: manifest.base_ref,
  });
  const expectedFiles = new Map(manifest.files.map((entry) => [entry.path, entry]));
  const actualFiles = new Map(actual.files.map((entry) => [entry.path, entry]));
  for (const [relative, expected] of expectedFiles) {
    const found = actualFiles.get(relative);
    if (!found) {
      throw new Error(`missing file: ${relative}`);
    }
    if (found.bytes !== expected.bytes || found.sha256 !== expected.sha256) {
      throw new Error(`changed file: ${relative}`);
    }
  }
  for (const relative of actualFiles.keys()) {
    if (!expectedFiles.has(relative)) {
      throw new Error(`unexpected file: ${relative}`);
    }
  }
  if (actual.file_count !== manifest.file_count || actual.byte_count !== manifest.byte_count) {
    throw new Error("manifest aggregate mismatch");
  }
  return manifest;
}

function assertManifestBinding(manifest, { expectedProjectRoot, expectedBaseRef } = {}) {
  if (manifest.schema_version !== 1 || !Array.isArray(manifest.files)) {
    throw new Error("unsupported snapshot manifest");
  }
  if (expectedProjectRoot && manifest.project_root !== path.resolve(expectedProjectRoot)) {
    throw new Error(`snapshot project_root mismatch: ${manifest.project_root}`);
  }
  if (expectedBaseRef && manifest.base_ref !== expectedBaseRef) {
    throw new Error(`snapshot base_ref mismatch: ${manifest.base_ref}`);
  }
}

export async function readAndVerifyManifest(snapshotRoot, expected = {}) {
  const source = path.resolve(snapshotRoot);
  const manifest = JSON.parse(await readFile(path.join(source, MANIFEST_FILE), "utf8"));
  assertManifestBinding(manifest, expected);
  await verifyTreeAgainstManifest(source, manifest);
  return manifest;
}

export async function verifyReadySnapshot({
  snapshotRoot,
  allowedSnapshotRoot,
  expected,
  expectedProjectRoot,
  expectedBaseRef,
}) {
  const source = assertContainedPath(snapshotRoot, allowedSnapshotRoot, "snapshot");
  const manifest = await readAndVerifyManifest(source, { expectedProjectRoot, expectedBaseRef });
  if (expected && JSON.stringify(manifest) !== JSON.stringify(expected)) {
    throw new Error("snapshot manifest does not match expected manifest");
  }
  return manifest;
}

export async function createReadySnapshot({
  liveCache,
  snapshotRoot,
  allowedCacheRoot,
  allowedSnapshotRoot,
  projectRoot,
  baseRef,
}) {
  const source = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  const destination = assertContainedPath(snapshotRoot, allowedSnapshotRoot, "snapshot");
  assertDisjointPaths(source, destination);
  await assertNoWriterArtifacts(source);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyRegularTree(source, destination);
  const manifest = await buildManifest(destination, { projectRoot, baseRef });
  await writeFile(path.join(destination, MANIFEST_FILE), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  await verifyReadySnapshot({
    snapshotRoot: destination,
    allowedSnapshotRoot,
    expected: manifest,
  });
  return manifest;
}

export async function restoreReadySnapshot({
  snapshotRoot,
  liveCache,
  allowedCacheRoot,
  allowedSnapshotRoot,
  expectedProjectRoot,
  expectedBaseRef,
}) {
  const source = assertContainedPath(snapshotRoot, allowedSnapshotRoot, "snapshot");
  const destination = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  assertDisjointPaths(source, destination);
  const manifest = await readAndVerifyManifest(source, { expectedProjectRoot, expectedBaseRef });
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyRegularTree(source, destination, { exclude: [MANIFEST_FILE] });
  await verifyTreeAgainstManifest(destination, manifest);
  return manifest;
}
