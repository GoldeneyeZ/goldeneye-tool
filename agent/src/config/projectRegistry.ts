import { mkdir, readFile, realpath, rename, rm, stat, writeFile } from "node:fs/promises";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { z } from "zod";

const registryEntrySchema = z.object({
  project: z.string().min(1),
  rootPath: z.string().min(1),
});

const registrySchema = z.object({
  version: z.literal(1),
  projects: z.array(registryEntrySchema),
});

export interface RegisteredProject {
  project: string;
  rootPath: string;
}

interface ProjectRegistry {
  version: 1;
  projects: RegisteredProject[];
}

export function projectRegistryPath(gcalHome: string): string {
  return join(gcalHome, "projects.json");
}

export async function canonicalDirectory(path: string): Promise<string> {
  const canonical = await realpath(path);
  const details = await stat(canonical);
  if (!details.isDirectory()) {
    throw new Error(`GCAL project path is not a directory: ${path}`);
  }
  return canonical;
}

export async function resolveRegisteredProject(
  currentDirectory: string,
  gcalHome: string,
): Promise<string | undefined> {
  const currentPath = await canonicalDirectory(currentDirectory);
  const registry = await readRegistry(gcalHome);
  const matches = registry.projects
    .filter((entry) => isSameOrChild(entry.rootPath, currentPath))
    .sort((left, right) => right.rootPath.length - left.rootPath.length);

  return matches[0]?.project;
}

export async function registerProject(
  gcalHome: string,
  registration: RegisteredProject,
): Promise<RegisteredProject> {
  const rootPath = await canonicalDirectory(registration.rootPath);
  const registry = await readRegistry(gcalHome);
  const projects = registry.projects.filter(
    (entry) =>
      comparePath(entry.rootPath) !== comparePath(rootPath) && entry.project !== registration.project,
  );
  projects.push({ project: registration.project, rootPath });
  projects.sort((left, right) => comparePath(left.rootPath).localeCompare(comparePath(right.rootPath)));

  await writeRegistry(gcalHome, { version: 1, projects });
  return { project: registration.project, rootPath };
}

async function readRegistry(gcalHome: string): Promise<ProjectRegistry> {
  const path = projectRegistryPath(gcalHome);
  let content: string;
  try {
    content = await readFile(path, "utf8");
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return { version: 1, projects: [] };
    }
    throw error;
  }

  try {
    return registrySchema.parse(JSON.parse(content));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Invalid GCAL project registry ${path}: ${message}`);
  }
}

async function writeRegistry(gcalHome: string, registry: ProjectRegistry): Promise<void> {
  const path = projectRegistryPath(gcalHome);
  const temporaryPath = `${path}.${process.pid}.${Date.now()}.tmp`;
  await mkdir(gcalHome, { recursive: true });

  try {
    await writeFile(temporaryPath, `${JSON.stringify(registry)}\n`, { encoding: "utf8", flag: "wx" });
    await rename(temporaryPath, path);
  } finally {
    await rm(temporaryPath, { force: true });
  }
}

function isSameOrChild(rootPath: string, candidatePath: string): boolean {
  const child = relative(rootPath, candidatePath);
  return child === "" || (child !== ".." && !child.startsWith(`..${sep}`) && !isAbsolute(child));
}

function comparePath(path: string): string {
  const normalized = resolve(path);
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}
