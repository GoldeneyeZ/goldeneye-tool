#!/usr/bin/env node
import { fileURLToPath } from "node:url";
import { existsSync, writeFileSync } from "node:fs";
import path from "node:path";
import { checksumText, fileRecords, walkFiles } from "./lib-assets.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.resolve(
  process.argv[2] ?? path.join(root, "../.upstream/codebase-memory-mcp/graph-ui"),
);
if (!existsSync(source)) throw new Error(`upstream graph UI not found: ${source}`);

const files = walkFiles(source, (file, entry) =>
  file.split("/").some((part) => part === "node_modules" || part === "dist" || part === ".git") ||
  entry.name.endsWith(".tsbuildinfo"),
);
const output = checksumText(fileRecords(source, files));
writeFileSync(path.join(root, "UPSTREAM_SOURCES.sha256"), output);
console.log(`captured ${files.length} upstream source assets`);
