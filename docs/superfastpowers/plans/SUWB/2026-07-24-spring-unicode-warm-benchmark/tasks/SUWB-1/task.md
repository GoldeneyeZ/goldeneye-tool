# Task 1: Build immutable snapshot primitives

<TASK-ID>SUWB-1</TASK-ID>

**Files:**

- Create: `tools/agent-bench/snapshot.mjs`
- Create: `tools/agent-bench/snapshot.test.mjs`

**Step 1: Write failing containment and unsafe-entry tests**

Create tests covering:

```js
test("assertContainedPath accepts descendants and rejects roots or escapes", () => {
  assert.doesNotThrow(() =>
    assertContainedPath("D:\\bench\\cache\\live", "D:\\bench\\cache", "live cache"));
  assert.throws(
    () => assertContainedPath("D:\\bench\\cache", "D:\\bench\\cache", "live cache"),
    /must be a strict descendant/);
  assert.throws(
    () => assertContainedPath("D:\\bench\\other", "D:\\bench\\cache", "live cache"),
    /must be a strict descendant/);
});

test("createReadySnapshot rejects symbolic links and writer artifacts", async () => {
  // Build temporary live cache with normal file, then add one symlink or
  // `goldeneye.db-wal`; each form must reject snapshot creation.
});
```

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: FAIL because `snapshot.mjs` does not exist.

**Step 2: Implement strict path and quiescence checks**

Implement and export:

```js
export function assertContainedPath(candidate, allowedRoot, label) {
  const root = path.resolve(allowedRoot);
  const target = path.resolve(candidate);
  const relative = path.relative(root, target);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`${label} must be a strict descendant of ${root}: ${target}`);
  }
  return target;
}

export async function assertNoWriterArtifacts(root) {
  const forbidden = /(?:^|[\\/])(?:goldeneye\.db-(?:wal|shm)|.*\.(?:lock|lck))$/i;
  const entries = await collectRegularFiles(root, { rejectSymlinks: true });
  const hit = entries.find(({ absolute }) => forbidden.test(absolute));
  if (hit) {
    throw new Error(`snapshot source is not quiescent: ${hit.absolute}`);
  }
}
```

`collectRegularFiles()` must:

- use `lstat()`;
- reject symbolic links, junctions, devices, sockets, and non-regular files;
- traverse directories in ordinal sorted order;
- return relative paths normalized with `/`;
- never follow links.

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: containment and unsafe-entry tests PASS.

**Step 3: Write failing manifest, isolation, and tamper tests**

Add tests proving:

- snapshot creation copies bytes rather than hardlinking;
- manifest file order is stable;
- SHA-256, byte count, and file count match;
- restoring creates a live copy with the same manifest;
- changing the restored live file leaves snapshot bytes unchanged;
- changing snapshot bytes causes verification failure;
- unexpected extra live file causes verification failure;
- source and destination outside configured roots fail before deletion/copy.

Use this manifest schema:

```json
{
  "schema_version": 1,
  "project_root": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "file_count": 1,
  "byte_count": 123,
  "files": [
    {
      "path": "ack-state/goldeneye.db",
      "bytes": 123,
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  ]
}
```

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: FAIL because snapshot creation, verification, and restore APIs are absent.

**Step 4: Implement copy-only snapshot lifecycle**

Implement:

```js
export async function createReadySnapshot({
  liveCache,
  snapshotRoot,
  allowedCacheRoot,
  projectRoot,
  baseRef,
}) {
  const source = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  const destination = assertContainedPath(snapshotRoot, allowedCacheRoot, "snapshot");
  await assertNoWriterArtifacts(source);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyTree(source, destination);
  const manifest = await buildManifest(destination, { projectRoot, baseRef });
  await writeFile(
    path.join(destination, "snapshot-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );
  await verifyReadySnapshot({ snapshotRoot: destination, expected: manifest });
  return manifest;
}

export async function restoreReadySnapshot({
  snapshotRoot,
  liveCache,
  allowedCacheRoot,
  expectedProjectRoot,
  expectedBaseRef,
}) {
  const source = assertContainedPath(snapshotRoot, allowedCacheRoot, "snapshot");
  const destination = assertContainedPath(liveCache, allowedCacheRoot, "live cache");
  const manifest = await readAndVerifyManifest(source, {
    expectedProjectRoot,
    expectedBaseRef,
  });
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyTree(source, destination, { exclude: ["snapshot-manifest.json"] });
  await verifyTreeAgainstManifest(destination, manifest);
  return manifest;
}
```

Important:

- Use `copyFile()` only; never `link()`, `copy-on-write`, or directory junctions.
- Exclude `snapshot-manifest.json` from the data-file manifest.
- Verify snapshot before copying and live cache after copying.
- Treat any absent, extra, changed, or unsafe file as a hard error.
- Record `project_root` and `base_ref` and compare exact normalized values.

Run:

```powershell
node --test tools/agent-bench/snapshot.test.mjs
```

Expected: all tests PASS.

**Step 5: Commit only Task 1 files**

```powershell
git add -- tools/agent-bench/snapshot.mjs tools/agent-bench/snapshot.test.mjs
git diff --cached --check
git commit -m "bench: add immutable ready snapshot primitives"
```

Expected: commit contains only two Task 1 files.
