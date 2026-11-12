#!/usr/bin/env node
import { homedir } from "node:os";
import { join } from "node:path";
import { createBenchmarkClient } from "./adapters/benchmark/createBenchmarkClient.js";
import { createGoldeneyeClient } from "./adapters/goldeneye/createGoldeneyeClient.js";
import { runCli } from "./cli/runCli.js";

const gcalHome = process.env.GCAL_HOME ?? join(homedir(), ".gcal");
const exitCode = await runCli({
  argv: process.argv,
  env: process.env,
  createClient: ({ backend, command, mcpUrl, project }) => {
    if (backend === "goldeneye") {
      return createGoldeneyeClient({
        gcalHome,
        command,
        project,
        env: process.env,
        writeWarning: (text) => process.stderr.write(text),
      });
    }

    return createBenchmarkClient({ command, mcpUrl, project });
  },
  writeOut: (text) => process.stdout.write(text),
  writeErr: (text) => process.stderr.write(text),
});

if (exitCode !== 0) {
  process.exit(exitCode);
}
