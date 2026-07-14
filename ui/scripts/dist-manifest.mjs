#!/usr/bin/env node
import { fileURLToPath } from "node:url";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { checksumText, fileRecords, sha256, walkFiles } from "./lib-assets.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");
for (const required of [
  "index.html",
  "runtime-config.js",
  "legal/LICENSE",
  "legal/UPSTREAM_LICENSE",
  "legal/THIRD_PARTY_LICENSES.md",
]) {
  if (!existsSync(path.join(dist, ...required.split("/")))) {
    throw new Error(`built UI is missing ${required}`);
  }
}

const sourceManifestHash = sha256(readFileSync(path.join(root, "asset-manifest.json")));
const files = walkFiles(dist, (file) => file === "asset-manifest.json" || file === "checksums.sha256");
const records = fileRecords(dist, files);
if (!records.some((record) => record.path.startsWith("assets/") && record.path.endsWith(".js"))) {
  throw new Error("built UI contains no JavaScript asset");
}
if (!records.some((record) => record.path.startsWith("assets/") && record.path.endsWith(".css"))) {
  throw new Error("built UI contains no CSS asset");
}

const manifest = JSON.stringify({
  schema: 1,
  name: "goldeneye-graph-ui-dist",
  sourceManifestSha256: sourceManifestHash,
  files: records,
}, null, 2) + "\n";
records.push({ path: "asset-manifest.json", bytes: Buffer.byteLength(manifest), sha256: sha256(manifest) });
const checksums = checksumText(records.sort((a, b) => a.path.localeCompare(b.path, "en")));
const manifestPath = path.join(dist, "asset-manifest.json");
const checksumsPath = path.join(dist, "checksums.sha256");
if (process.argv.includes("--check")) {
  if (readFileSync(manifestPath, "utf8") !== manifest || readFileSync(checksumsPath, "utf8") !== checksums) {
    throw new Error("built UI asset manifest is stale");
  }
  console.log(`verified ${records.length} built assets`);
} else {
  writeFileSync(manifestPath, manifest);
  writeFileSync(checksumsPath, checksums);
  console.log(`recorded ${records.length} built assets`);
}
