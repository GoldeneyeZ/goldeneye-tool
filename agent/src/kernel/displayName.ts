import type { GraphNode } from "./codeGraph.js";

// Backend identifiers embed the project and the file path
// (`<project>.<dir>.<dir>.<File>.<Type>.<member>`). The path is printed next to
// every row anyway, so the short form keeps only what follows the file segment.
// `gcal get` resolves the short form back, see resolveSymbol.

export function displayName(node: GraphNode, project: string): string {
  if (node.qualifiedName.startsWith("__file__") || node.label === "File") {
    return node.filePath || node.name;
  }

  const pathSegments = node.filePath.split("/").filter((segment) => segment.length > 0);
  const prefix = `${project}.`;
  const local = node.qualifiedName.startsWith(prefix)
    ? node.qualifiedName.slice(prefix.length)
    : node.qualifiedName;
  const segments = local.split(".");
  const fileIndex = pathSegments.length - 1;

  if (pathSegments.length > 0 && segments.length >= pathSegments.length) {
    const stem = pathSegments[fileIndex].replace(/\.[^.]+$/, "");
    if (sameIdentifier(segments[fileIndex], stem)) {
      const rest = segments.slice(pathSegments.length);
      return rest.length > 0 ? rest.join(".") : segments[fileIndex];
    }
  }

  return segments.slice(-2).join(".");
}

export function location(node: GraphNode): string {
  if (node.startLine <= 0) return node.filePath;
  return node.endLine > node.startLine
    ? `${node.filePath}:${node.startLine}-${node.endLine}`
    : `${node.filePath}:${node.startLine}`;
}

export function isTestPath(filePath: string): boolean {
  return (
    /(^|\/)(tests?|__tests__|spec|specs|testing|testdata|fixtures)\//i.test(filePath) ||
    /(Tests?|Spec|IT)\.[A-Za-z]+$/.test(filePath) ||
    /[._-](test|spec)\.[A-Za-z]+$/i.test(filePath)
  );
}

function sameIdentifier(left: string, right: string): boolean {
  const canonical = (value: string) => value.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
  return canonical(left) === canonical(right);
}
