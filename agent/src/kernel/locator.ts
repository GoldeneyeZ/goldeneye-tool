// Parses what an agent types to point at code. Agents naturally write
// `Class.method`, a package-qualified name, `Class#method`, `path/File.ext`,
// `File.ext:42`, or `File.ext:10-60`; all of them are accepted.

export interface SymbolLocator {
  kind: "symbol";
  raw: string;
  segments: string[];
  /** 1-based overload index from a trailing `#n`, when given. */
  overload: number | undefined;
}

export interface FileLocator {
  kind: "file";
  raw: string;
  path: string;
  startLine: number | undefined;
  endLine: number | undefined;
}

export type Locator = SymbolLocator | FileLocator;

const LINE_SUFFIX = /^(.*?):(\d+)(?:-(\d+))?$/;

export function parseLocator(raw: string): Locator {
  const text = raw.trim();
  const lineMatch = LINE_SUFFIX.exec(text);
  if (lineMatch !== null && !/^[A-Za-z]$/.test(lineMatch[1])) {
    const startLine = Number(lineMatch[2]);
    const endLine = lineMatch[3] === undefined ? undefined : Number(lineMatch[3]);
    return { kind: "file", raw: text, path: normalizePath(lineMatch[1]), startLine, endLine };
  }

  if (/[\\/]/.test(text)) {
    return { kind: "file", raw: text, path: normalizePath(text), startLine: undefined, endLine: undefined };
  }

  return parseSymbolLocator(text);
}

export function parseSymbolLocator(raw: string): SymbolLocator {
  let text = raw.trim().replace(/\(.*\)$/s, "");
  let overload: number | undefined;
  const overloadMatch = /#(\d+)$/.exec(text);
  if (overloadMatch !== null) {
    overload = Number(overloadMatch[1]);
    text = text.slice(0, overloadMatch.index);
  }

  const segments = text
    .replace(/::|#/g, ".")
    .split(".")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  return { kind: "symbol", raw: raw.trim(), segments, overload };
}

/** A bare `Name.ext` may be a file or a member access; the caller decides. */
export function couldBeFileName(locator: SymbolLocator): boolean {
  return locator.overload === undefined && /^[^.\s]+(\.[^.\s]+)*\.[A-Za-z0-9]{1,8}$/.test(locator.raw);
}

export function normalizePath(path: string): string {
  return path.trim().replace(/\\/g, "/").replace(/^\.\//, "");
}
