import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  projectRegistryPath,
  registerProject,
  resolveRegisteredProject,
} from "../src/config/projectRegistry.js";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

async function temporaryDirectory(prefix: string): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), prefix));
  temporaryDirectories.push(path);
  return path;
}

describe("GCAL project registry", () => {
  it("registers projects under the user home and resolves descendants", async () => {
    const userHome = await temporaryDirectory("gcal-home-");
    const gcalHome = join(userHome, ".gcal");
    const projectRoot = await temporaryDirectory("gcal-project-");
    const nested = join(projectRoot, "src", "feature");
    await mkdir(nested, { recursive: true });

    await registerProject(gcalHome, { project: "example-project", rootPath: projectRoot });

    await expect(resolveRegisteredProject(nested, gcalHome)).resolves.toBe("example-project");
    await expect(readFile(projectRegistryPath(gcalHome), "utf8")).resolves.toBe(
      `${JSON.stringify({
        version: 1,
        projects: [{ project: "example-project", rootPath: projectRoot }],
      })}\n`,
    );
  });

  it("uses the most specific registered ancestor", async () => {
    const userHome = await temporaryDirectory("gcal-home-");
    const gcalHome = join(userHome, ".gcal");
    const parentRoot = await temporaryDirectory("gcal-parent-");
    const childRoot = join(parentRoot, "packages", "child");
    const nested = join(childRoot, "src");
    await mkdir(nested, { recursive: true });

    await registerProject(gcalHome, { project: "parent-project", rootPath: parentRoot });
    await registerProject(gcalHome, { project: "child-project", rootPath: childRoot });

    await expect(resolveRegisteredProject(nested, gcalHome)).resolves.toBe("child-project");
  });

  it("returns undefined when no registry or ancestor match exists", async () => {
    const userHome = await temporaryDirectory("gcal-home-");
    const gcalHome = join(userHome, ".gcal");
    const currentDirectory = await temporaryDirectory("gcal-unregistered-");

    await expect(resolveRegisteredProject(currentDirectory, gcalHome)).resolves.toBeUndefined();
  });

  it("reports malformed registry content with its path", async () => {
    const userHome = await temporaryDirectory("gcal-home-");
    const gcalHome = join(userHome, ".gcal");
    const currentDirectory = await temporaryDirectory("gcal-project-");
    await mkdir(gcalHome, { recursive: true });
    await writeFile(projectRegistryPath(gcalHome), "not-json", "utf8");

    await expect(resolveRegisteredProject(currentDirectory, gcalHome)).rejects.toThrow(
      `Invalid GCAL project registry ${projectRegistryPath(gcalHome)}`,
    );
  });
});
