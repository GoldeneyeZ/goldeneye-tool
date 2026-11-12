import { resolve } from "node:path";
import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type { IndexedProject } from "../domain/types.js";
import {
  canonicalDirectory,
  registerProject,
  type RegisteredProject,
} from "../config/projectRegistry.js";

export async function initializeProject(
  client: GcalBackendClient,
  repoPath: string,
  currentDirectory: string,
  gcalHome: string,
): Promise<RegisteredProject> {
  const rootPath = await canonicalDirectory(resolve(currentDirectory, repoPath));
  await client.index(rootPath);
  const indexedProjects = await client.projects();
  const indexedProject = findProjectByRoot(indexedProjects, rootPath);

  if (indexedProject === undefined) {
    throw new Error(`Goldeneye indexed ${rootPath}, but list_projects did not return that root`);
  }

  return registerProject(gcalHome, { project: indexedProject.name, rootPath });
}

function findProjectByRoot(
  projects: IndexedProject[],
  rootPath: string,
): IndexedProject | undefined {
  const expected = comparablePath(rootPath);
  return projects.find((project) => comparablePath(project.rootPath) === expected);
}

function comparablePath(path: string): string {
  const normalized = resolve(path);
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}
