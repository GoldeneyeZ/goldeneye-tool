import assert from "node:assert/strict";
import { copyFile, readFile, mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import * as snapshot from "./snapshot.mjs";

async function makeFixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), "agent-bench-snapshot-"));
  const allowedCacheRoot = path.join(root, "allowed-cache");
  const allowedSnapshotRoot = path.join(root, "allowed-snapshots");
  const liveCache = path.join(allowedCacheRoot, "live-cache");
  const snapshotRoot = path.join(allowedSnapshotRoot, "ready-snapshot");
  await mkdir(path.join(liveCache, "ack-state"), { recursive: true });
  await writeFile(path.join(liveCache, "ack-state", "goldeneye.db"), "database-bytes");
  await writeFile(path.join(liveCache, "config.json"), "{\"cache\":true}\n");
  t.after(() => rm(root, { recursive: true, force: true }));
  return {
    root,
    allowedCacheRoot,
    allowedSnapshotRoot,
    liveCache,
    snapshotRoot,
    projectRoot: path.join(root, "stable-worktree"),
    baseRef: "daf955157871e4ac6f192e06b71d6cc595eb979b",
  };
}

test("snapshot API exposes creation, verification, restore, and containment", () => {
  assert.equal(typeof snapshot.assertContainedPath, "function");
  assert.equal(typeof snapshot.checkpointSqliteDatabase, "function");
  assert.equal(typeof snapshot.createReadySnapshot, "function");
  assert.equal(typeof snapshot.verifyReadySnapshot, "function");
  assert.equal(typeof snapshot.restoreReadySnapshot, "function");
  assert.equal(typeof snapshot.verifyTreeAgainstManifest, "function");
});

test("checkpointSqliteDatabase closes a persistent WAL through SQLite", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "agent-bench-sqlite-"));
  const databasePath = path.join(root, "goldeneye.db");
  const savedWal = path.join(root, "saved-wal");
  const savedShm = path.join(root, "saved-shm");
  const database = new DatabaseSync(databasePath);
  database.exec(`
    PRAGMA journal_mode = WAL;
    PRAGMA wal_autocheckpoint = 0;
    CREATE TABLE evidence(value TEXT NOT NULL);
    INSERT INTO evidence VALUES ('preserved');
  `);
  await copyFile(`${databasePath}-wal`, savedWal);
  await copyFile(`${databasePath}-shm`, savedShm);
  database.close();
  await copyFile(savedWal, `${databasePath}-wal`);
  await copyFile(savedShm, `${databasePath}-shm`);
  await rm(savedWal);
  await rm(savedShm);
  t.after(() => rm(root, { recursive: true, force: true }));

  await assert.rejects(() => snapshot.assertNoWriterArtifacts(root), /not quiescent/);
  const checkpoint = snapshot.checkpointSqliteDatabase(databasePath);
  assert.equal(checkpoint.busy, 0);
  assert.equal(checkpoint.closed, true);
  assert.equal(checkpoint.sidecars_absent, true);
  await snapshot.assertNoWriterArtifacts(root);

  const verified = new DatabaseSync(databasePath, { readOnly: true });
  assert.equal(verified.prepare("SELECT value FROM evidence").get().value, "preserved");
  verified.close();
});

test("assertContainedPath accepts only strict descendants", async (t) => {
  const fixture = await makeFixture(t);
  assert.equal(
    snapshot.assertContainedPath(fixture.liveCache, fixture.allowedCacheRoot, "live cache"),
    path.resolve(fixture.liveCache),
  );
  assert.throws(
    () => snapshot.assertContainedPath(fixture.allowedCacheRoot, fixture.allowedCacheRoot, "cache root"),
    /strict descendant/,
  );
  assert.throws(
    () => snapshot.assertContainedPath(path.join(fixture.root, "outside"), fixture.allowedCacheRoot, "outside"),
    /strict descendant/,
  );
});

test("createReadySnapshot copies sorted bytes and writes a stable manifest", async (t) => {
  const fixture = await makeFixture(t);
  const manifest = await snapshot.createReadySnapshot(fixture);
  assert.deepEqual(manifest, {
    schema_version: 1,
    project_root: path.resolve(fixture.projectRoot),
    base_ref: fixture.baseRef,
    file_count: 2,
    byte_count: Buffer.byteLength("database-bytes") + Buffer.byteLength("{\"cache\":true}\n"),
    files: [
      {
        path: "ack-state/goldeneye.db",
        bytes: Buffer.byteLength("database-bytes"),
        sha256: "f04766726302b22889d791aa0fe6e41e7342dc71b62ec234dafdc1364d559027",
      },
      {
        path: "config.json",
        bytes: Buffer.byteLength("{\"cache\":true}\n"),
        sha256: "1961a91054f256e8eb021fdef318262bfa221403ae6b9e40e4196eef18e0ac9f",
      },
    ],
  });
  await snapshot.verifyReadySnapshot({
    snapshotRoot: fixture.snapshotRoot,
    allowedSnapshotRoot: fixture.allowedSnapshotRoot,
    expected: manifest,
  });
  const saved = JSON.parse(await readFile(path.join(fixture.snapshotRoot, "snapshot-manifest.json"), "utf8"));
  assert.deepEqual(saved, manifest);
});

test("restoreReadySnapshot makes independent live bytes and verifies manifest parity", async (t) => {
  const fixture = await makeFixture(t);
  const manifest = await snapshot.createReadySnapshot(fixture);
  await writeFile(path.join(fixture.liveCache, "config.json"), "changed-before-restore");
  const restored = await snapshot.restoreReadySnapshot({
    snapshotRoot: fixture.snapshotRoot,
    liveCache: fixture.liveCache,
    allowedCacheRoot: fixture.allowedCacheRoot,
    allowedSnapshotRoot: fixture.allowedSnapshotRoot,
    expectedProjectRoot: fixture.projectRoot,
    expectedBaseRef: fixture.baseRef,
  });
  assert.deepEqual(restored, manifest);
  assert.equal(await readFile(path.join(fixture.liveCache, "config.json"), "utf8"), "{\"cache\":true}\n");
  await writeFile(path.join(fixture.liveCache, "config.json"), "mutated-live-copy");
  assert.equal(await readFile(path.join(fixture.snapshotRoot, "config.json"), "utf8"), "{\"cache\":true}\n");
  await assert.rejects(
    () => snapshot.verifyTreeAgainstManifest(fixture.liveCache, manifest),
    /changed file/,
  );
  await writeFile(path.join(fixture.liveCache, "config.json"), "{\"cache\":true}\n");
  await writeFile(path.join(fixture.liveCache, "unexpected.txt"), "extra");
  await assert.rejects(
    () => snapshot.verifyTreeAgainstManifest(fixture.liveCache, manifest),
    /unexpected file/,
  );
});

test("createReadySnapshot rejects unsafe source entries and writer artifacts", async (t) => {
  const fixture = await makeFixture(t);
  await writeFile(path.join(fixture.liveCache, "goldeneye.db-wal"), "active writer");
  await assert.rejects(() => snapshot.createReadySnapshot(fixture), /not quiescent/);
  await rm(path.join(fixture.liveCache, "goldeneye.db-wal"));
  try {
    await symlink(path.join(fixture.liveCache, "config.json"), path.join(fixture.liveCache, "linked-config"));
  }
  catch (error) {
    if (error.code !== "EPERM") {
      throw error;
    }
    const junctionTarget = path.join(fixture.root, "junction-target");
    await mkdir(junctionTarget);
    await symlink(junctionTarget, path.join(fixture.liveCache, "linked-config"), "junction");
  }
  await assert.rejects(() => snapshot.createReadySnapshot(fixture), /unsafe filesystem entry/);
});

test("snapshot verification rejects changed bytes and wrong snapshot roots before mutation", async (t) => {
  const fixture = await makeFixture(t);
  const manifest = await snapshot.createReadySnapshot(fixture);
  await writeFile(path.join(fixture.snapshotRoot, "config.json"), "tampered");
  await assert.rejects(
    () => snapshot.verifyReadySnapshot({
      snapshotRoot: fixture.snapshotRoot,
      allowedSnapshotRoot: fixture.allowedSnapshotRoot,
      expected: manifest,
    }),
    /changed file/,
  );

  const outsideSnapshot = path.join(fixture.allowedCacheRoot, "outside-snapshot");
  await mkdir(outsideSnapshot);
  await writeFile(path.join(outsideSnapshot, "preserve.txt"), "must-survive");
  await assert.rejects(
    () => snapshot.createReadySnapshot({ ...fixture, snapshotRoot: outsideSnapshot }),
    /strict descendant/,
  );
  assert.equal(await readFile(path.join(outsideSnapshot, "preserve.txt"), "utf8"), "must-survive");
});

test("restoreReadySnapshot rejects snapshot outside configured root before deleting live cache", async (t) => {
  const fixture = await makeFixture(t);
  const wrongSnapshotRoot = path.join(fixture.allowedCacheRoot, "wrong-snapshot");
  await snapshot.createReadySnapshot({
    ...fixture,
    snapshotRoot: wrongSnapshotRoot,
    allowedSnapshotRoot: fixture.allowedCacheRoot,
  });
  await writeFile(path.join(fixture.liveCache, "config.json"), "preserve-live-cache");
  await assert.rejects(
    () => snapshot.restoreReadySnapshot({
      snapshotRoot: wrongSnapshotRoot,
      liveCache: fixture.liveCache,
      allowedCacheRoot: fixture.allowedCacheRoot,
      allowedSnapshotRoot: fixture.allowedSnapshotRoot,
      expectedProjectRoot: fixture.projectRoot,
      expectedBaseRef: fixture.baseRef,
    }),
    /strict descendant/,
  );
  assert.equal(await readFile(path.join(fixture.liveCache, "config.json"), "utf8"), "preserve-live-cache");
});
