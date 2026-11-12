import {
  findByExactName,
  findByName,
  findBySegments,
  findBySimilarName,
  isContainerNode,
  isTypeNode,
  type GraphNode,
  type ToolClient,
} from "../kernel/codeGraph.js";
import { isTestPath } from "../kernel/displayName.js";
import type { SymbolLocator } from "../kernel/locator.js";

export type SymbolResolution =
  /**
   * One symbol; several nodes only when they are overloads of it.
   * `declarations`: where the same symbol is only declared (a C prototype, a
   * forward declaration), left out in favour of its definition.
   */
  | { kind: "resolved"; nodes: GraphNode[]; declarations?: GraphNode[] }
  | { kind: "ambiguous"; nodes: GraphNode[] }
  /** `similar`: the suggestions are near-miss names, not same-named symbols elsewhere. */
  | { kind: "not-found"; suggestions: GraphNode[]; similar?: boolean };

export async function resolveSymbol(
  client: ToolClient,
  project: string,
  locator: SymbolLocator,
): Promise<SymbolResolution> {
  if (locator.segments.length === 0) return { kind: "not-found", suggestions: [] };

  if (locator.raw.startsWith(`${project}.`)) {
    const exact = await findByExactName(client, locator.raw);
    if (exact.length > 0) return { kind: "resolved", nodes: exact };
  }

  // Leading segments an agent guessed wrong (a package, a module) are dropped
  // one at a time; the trailing segments carry the identity.
  for (let start = 0; start < locator.segments.length; start += 1) {
    const segments = locator.segments.slice(start);
    if (start > 0 && segments.length < 2) break;
    const found = select(await findBySegments(client, segments, locator.overload));
    if (found !== undefined) return found;
  }

  const last = locator.segments[locator.segments.length - 1];
  const byName = (await findByName(client, last)).filter((node) => !isContainerNode(node));
  if (locator.segments.length === 1) {
    const found = select(byName);
    if (found !== undefined) return found;
  }
  if (locator.segments.length >= 2 && byName.length > 0) {
    const owner = locator.segments[locator.segments.length - 2];
    const beside = await besideOwner(client, owner, byName);
    const found = select(beside.members);
    if (found?.kind === "resolved") return found;
    // Several candidates in the package: the one in the owner's own file wins.
    const inOwnerFile = beside.members.filter((node) => beside.ownerFiles.has(node.filePath));
    if (inOwnerFile.length > 0 && inOwnerFile.length < beside.members.length) {
      const narrowed = select(inOwnerFile);
      if (narrowed?.kind === "resolved") return narrowed;
    }

    // A module-level symbol has no owner type to name; its file stands in:
    // `native.ts.native`, or `native.native` for the one in native.ts.
    const inFile = select(inNamedFile(byName, locator.segments.slice(0, -1).join(".")));
    if (inFile?.kind === "resolved") return inFile;
  }
  if (byName.length > 0) return { kind: "not-found", suggestions: byName };

  // Nothing is named like that: offer names that differ by a few characters
  // (a plural, a missing word separator), types first.
  const similar = (await findBySimilarName(client, last)).filter((node) => !isContainerNode(node));
  const types = similar.filter(isTypeNode);
  return { kind: "not-found", suggestions: types.length > 0 ? types : similar, similar: true };
}

const BESIDE_OWNER_MAX = 8;

/**
 * Go indexes a method under its package, not its receiver: `Engine.run` is
 * `sync.run`. Among the symbols named like the member, those that sit in the
 * directory of a type named `owner` and belong to no type themselves are the
 * owner's methods.
 */
async function besideOwner(
  client: ToolClient,
  owner: string,
  candidates: GraphNode[],
): Promise<{ members: GraphNode[]; ownerFiles: Set<string> }> {
  const owners = (await findByName(client, owner)).filter(isTypeNode);
  const ownerFiles = new Set(owners.map((node) => node.filePath));
  if (owners.length === 0) return { members: [], ownerFiles };

  const directoryOf = (filePath: string) => filePath.slice(0, filePath.lastIndexOf("/") + 1);
  const directories = new Set(owners.map((node) => directoryOf(node.filePath)));
  const beside = candidates.filter(
    (node) => !isTypeNode(node) && directories.has(directoryOf(node.filePath)),
  );

  const members: GraphNode[] = [];
  for (const node of beside.slice(0, BESIDE_OWNER_MAX)) {
    const parent = node.qualifiedName.replace(/#\d+$/, "").split(".").slice(0, -1).join(".");
    const parents = parent === "" ? [] : await findByExactName(client, parent);
    if (!parents.some(isTypeNode)) members.push(node);
  }
  return { members, ownerFiles };
}

/** Candidates declared in the file named `file` (with or without its extension), outermost first. */
function inNamedFile(candidates: GraphNode[], file: string): GraphNode[] {
  const named = candidates.filter((node) => {
    const base = node.filePath.split("/").pop() ?? "";
    return base === file || base.replace(/\.[^.]+$/, "") === file;
  });
  const outermost = Math.min(...named.map(nesting));
  return named.filter((node) => nesting(node) === outermost);
}

/** The one symbol a list of same-named candidates stands for, when there is exactly one. */
export function soleSymbol(candidates: GraphNode[]): GraphNode[] | undefined {
  const found = select(candidates);
  return found?.kind === "resolved" ? found.nodes : undefined;
}

const C_FAMILY_FILE = /\.(c|h|cc|cpp|cxx|hh|hpp|hxx|m|mm)$/i;
const C_FILE = /\.(c|h)$/i;
const HEADER_FILE = /\.(h|hh|hpp|hxx)$/i;

/** How deep the symbol sits below its file: 1 for a top-level declaration. */
function nesting(node: GraphNode): number {
  const canonical = (value: string) => value.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
  const stem = canonical((node.filePath.split("/").pop() ?? "").replace(/\.[^.]+$/, ""));
  const segments = node.qualifiedName.replace(/#\d+$/, "").split(".");
  for (let index = segments.length - 2; index >= 0; index -= 1) {
    if (canonical(segments[index]) === stem) return segments.length - 1 - index;
  }
  return 1;
}

/**
 * C and C++ name one symbol in several places: a prototype in a header, a
 * forward declaration, a struct mentioned inside another declaration. The
 * definition is what a reader asks for.
 */
function definitions(groups: GraphNode[][]): GraphNode[][] {
  let kept = groups;
  const narrow = (keep: (node: GraphNode) => boolean) => {
    const narrowed = kept.filter((group) => keep(group[0]));
    if (narrowed.length > 0) kept = narrowed;
  };
  narrow((node) => !(C_FILE.test(node.filePath) && isTypeNode(node) && nesting(node) > 1));
  narrow(
    (node) =>
      !(C_FAMILY_FILE.test(node.filePath) && isTypeNode(node) && node.endLine <= node.startLine),
  );
  narrow((node) => isTypeNode(node) || !HEADER_FILE.test(node.filePath));
  return kept;
}

function select(candidates: GraphNode[]): SymbolResolution | undefined {
  // One declaration can be indexed under two kinds (a Go struct is a Class
  // and a Struct); it is still one symbol.
  const declarations = new Set<string>();
  let nodes = candidates.filter((node) => {
    if (isContainerNode(node)) return false;
    const declaration = `${node.filePath}:${node.startLine}-${node.endLine}:${node.name}`;
    if (declarations.has(declaration)) return false;
    declarations.add(declaration);
    return true;
  });
  if (nodes.length === 0) return undefined;

  // `Foo` names the type, not its constructors `Foo.Foo`.
  const types = nodes.filter(isTypeNode);
  if (types.length > 0) {
    nodes = nodes.filter(
      (node) =>
        isTypeNode(node) ||
        !types.some(
          (type) => type.name === node.name && node.qualifiedName.startsWith(`${type.qualifiedName}.`),
        ),
    );
  }

  const groups = groupOverloads(nodes);
  if (groups.length === 1) return { kind: "resolved", nodes: groups[0] };

  const production = groups.filter((group) => !isTestPath(group[0].filePath));
  if (production.length === 1) return { kind: "resolved", nodes: production[0] };

  const pool = production.length > 0 ? production : groups;
  const defined = definitions(pool);
  if (defined.length === 1) {
    return {
      kind: "resolved",
      nodes: defined[0],
      declarations: pool.filter((group) => group !== defined[0]).map((group) => group[0]),
    };
  }

  return {
    kind: "ambiguous",
    nodes: (defined.length < pool.length ? defined : groups).map((group) => group[0]),
  };
}

function groupOverloads(nodes: GraphNode[]): GraphNode[][] {
  const groups = new Map<string, GraphNode[]>();
  for (const node of nodes) {
    const base = node.qualifiedName.replace(/#\d+$/, "");
    const group = groups.get(base);
    if (group === undefined) groups.set(base, [node]);
    else group.push(node);
  }
  return [...groups.values()].map((group) =>
    group.sort((left, right) => left.startLine - right.startLine),
  );
}
