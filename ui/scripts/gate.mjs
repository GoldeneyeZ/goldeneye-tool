import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const npmCli = process.env.npm_execpath;
const npm = npmCli ? process.execPath : "npm";
const npmArgs = npmCli ? [npmCli] : [];

const checks = [
  ["tests and source integrity", npm, [...npmArgs, "test"], root],
  ["production build", npm, [...npmArgs, "run", "build"], root],
  ["built asset integrity", process.execPath, ["scripts/dist-manifest.mjs", "--check"], root],
  ["whitespace", "git", ["diff", "--check", "--", "ui"], path.resolve(root, "..")],
];

for (const [label, command, args, cwd] of checks) {
  console.log(`\n[gate] ${label}`);
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.error) {
    console.error(`[gate] ${label}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`[gate] ${label}: failed (${result.status ?? "unknown"})`);
    process.exit(result.status ?? 1);
  }
}

console.log("\n[gate] all UI checks passed");
