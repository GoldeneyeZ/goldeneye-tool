#!/usr/bin/env node
import { fileURLToPath } from "node:url";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { checksumText, fileRecords, sha256, walkFiles } from "./lib-assets.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(root, "asset-manifest.json");
const checksumsPath = path.join(root, "checksums.sha256");
const generated = new Set(["asset-manifest.json", "checksums.sha256"]);
const ignoredDirectories = new Set(["node_modules", "dist", "coverage", ".git"]);

function excluded(file, entry) {
  const first = file.split("/")[0];
  return ignoredDirectories.has(first) || generated.has(file) || entry.name.endsWith(".tsbuildinfo");
}

function buildManifest() {
  const files = walkFiles(root, excluded);
  return {
    schema: 1,
    name: "goldeneye-graph-ui",
    upstream: {
      repository: "https://github.com/DeusData/codebase-memory-mcp",
      commit: "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c",
      source: "graph-ui/",
      checksums: "UPSTREAM_SOURCES.sha256",
    },
    excluded: ["asset-manifest.json", "checksums.sha256", "node_modules/", "dist/", "coverage/", "*.tsbuildinfo"],
    files: fileRecords(root, files),
  };
}

const manifest = JSON.stringify(buildManifest(), null, 2) + "\n";
const checksumRecords = fileRecords(
  root,
  walkFiles(root, (file, entry) => {
    const first = file.split("/")[0];
    return ignoredDirectories.has(first) || file === "checksums.sha256" || entry.name.endsWith(".tsbuildinfo");
  }),
).filter((record) => record.path !== "asset-manifest.json");
checksumRecords.push({
  path: "asset-manifest.json",
  bytes: Buffer.byteLength(manifest),
  sha256: sha256(manifest),
});
const checksums = checksumText(checksumRecords.sort((a, b) => a.path.localeCompare(b.path, "en")));

if (process.argv.includes("--check")) {
  const actualManifest = readFileSync(manifestPath, "utf8");
  const actualChecksums = readFileSync(checksumsPath, "utf8");
  if (actualManifest !== manifest || actualChecksums !== checksums) {
    throw new Error("UI asset manifest is stale; run npm run assets:generate");
  }
  console.log(`verified ${checksumRecords.length} source assets`);
} else {
  writeFileSync(manifestPath, manifest);
  writeFileSync(checksumsPath, checksums);
  console.log(`recorded ${checksumRecords.length} source assets`);
}
