#!/usr/bin/env node
import { fileURLToPath } from "node:url";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const lock = JSON.parse(readFileSync(path.join(root, "package-lock.json"), "utf8"));
const policy = JSON.parse(readFileSync(path.join(root, "license-policy.json"), "utf8"));
const allowed = new Set(policy.allowed_spdx_ids.map((id) => id.toLowerCase()));
const skipTokens = new Set(["and", "or", "with"]);
const headerLicenses = new Map([
  ["mit license", "MIT"],
  ["the mit license", "MIT"],
  ["the mit license (mit)", "MIT"],
  ["apache license", "Apache-2.0"],
  ["isc license", "ISC"],
  ["bsd 2-clause license", "BSD-2-Clause"],
  ["bsd 3-clause license", "BSD-3-Clause"],
  ["the unlicense", "Unlicense"],
]);

function packageName(lockPath, metadata) {
  return metadata.name ?? lockPath.slice(lockPath.lastIndexOf("node_modules/") + 13);
}

function licenseFile(directory) {
  const name = readdirSync(directory)
    .sort((a, b) => a.localeCompare(b, "en"))
    .find((file) => /^(licen[cs]e|copying|notice|unlicense)/i.test(file));
  return name ? readFileSync(path.join(directory, name), "utf8").trim() : null;
}

const packages = new Map();
for (const [lockPath, lockMetadata] of Object.entries(lock.packages ?? {})) {
  if (!lockPath.includes("node_modules/") || lockMetadata.dev === true) continue;
  const directory = path.join(root, ...lockPath.split("/"));
  const metadataPath = path.join(directory, "package.json");
  if (!existsSync(metadataPath)) continue;
  const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
  if (metadata.os || metadata.cpu) continue;
  const name = packageName(lockPath, metadata);
  const version = metadata.version ?? lockMetadata.version;
  const key = `${name}@${version}`;
  if (packages.has(key)) continue;
  let license = typeof metadata.license === "object" ? metadata.license?.type : metadata.license;
  const text = licenseFile(directory);
  if (!license && text) license = headerLicenses.get(text.split(/\r?\n/, 1)[0].trim().toLowerCase());
  packages.set(key, { name, version, license: String(license ?? ""), text });
}

if (packages.size === 0) throw new Error("production dependency tree is empty; run npm ci");
const violations = [];
for (const [key, metadata] of packages) {
  const tokens = metadata.license.match(/[A-Za-z0-9.+-]+/g) ?? [];
  if (tokens.length === 0) {
    violations.push(`${key}: no resolvable license`);
    continue;
  }
  if (tokens.some((token) => !skipTokens.has(token.toLowerCase()) && !allowed.has(token.toLowerCase()))) {
    violations.push(`${key}: ${metadata.license}`);
  }
}
if (violations.length) throw new Error(`license policy rejected:\n${violations.join("\n")}`);

const lines = [
  "# Goldeneye graph UI third-party licenses",
  "",
  "Generated deterministically from the installed production dependency tree.",
  "Build-only and platform-specific packages are excluded because their code is not embedded.",
  "",
];
for (const [key, metadata] of [...packages].sort(([a], [b]) => a.localeCompare(b, "en"))) {
  lines.push(`## ${key} — ${metadata.license}`, "");
  lines.push(metadata.text || `(no license file shipped; declared license: ${metadata.license})`, "");
}
const report = lines.join("\n").replace(/\r\n/g, "\n") + "\n";
const reportPath = path.join(root, "THIRD_PARTY_LICENSES.md");

if (process.argv.includes("--check")) {
  if (!existsSync(reportPath) || readFileSync(reportPath, "utf8") !== report) {
    throw new Error("third-party license report is stale; run npm run licenses:generate");
  }
  console.log(`verified ${packages.size} production package licenses`);
} else {
  writeFileSync(reportPath, report);
  console.log(`recorded ${packages.size} production package licenses`);
}
