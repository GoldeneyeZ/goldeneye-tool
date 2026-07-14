import { createHash } from "node:crypto";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";

export function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

export function walkFiles(root, excluded = () => false, relative = "") {
  const directory = path.join(root, relative);
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name, "en"),
  )) {
    const child = relative ? `${relative}/${entry.name}` : entry.name;
    if (excluded(child, entry)) continue;
    const absolute = path.join(root, ...child.split("/"));
    const info = lstatSync(absolute);
    if (info.isSymbolicLink()) throw new Error(`symbolic links are not allowed: ${child}`);
    if (info.isDirectory()) files.push(...walkFiles(root, excluded, child));
    else if (info.isFile()) files.push(child);
  }
  return files;
}

export function fileRecords(root, files) {
  return files.map((file) => {
    const content = readFileSync(path.join(root, ...file.split("/")));
    return { path: file, bytes: content.length, sha256: sha256(content) };
  });
}

export function checksumText(records) {
  return records.map((record) => `${record.sha256}  ${record.path}`).join("\n") + "\n";
}
