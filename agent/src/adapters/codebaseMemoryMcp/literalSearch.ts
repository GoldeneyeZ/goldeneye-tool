import type { SymbolCandidate } from "../../domain/types.js";

export type LiteralSearchBranch =
  | { kind: "query"; value: string }
  | { kind: "namePattern"; value: string };

const literalTokenPattern = /[\p{L}\p{N}_]+/gu;

export function compileLiteralSearch(input: string): LiteralSearchBranch[] {
  const branches: LiteralSearchBranch[] = [];
  const seen = new Set<string>();

  for (const rawBranch of input.split("|")) {
    const branch = compileBranch(rawBranch.trim());
    if (!branch) continue;

    const key = `${branch.kind}:${branch.value}`;
    if (seen.has(key)) continue;
    seen.add(key);
    branches.push(branch);
  }

  if (branches.length === 0) {
    throw new Error("GCAL search query must contain letters, numbers, or underscores");
  }

  return branches;
}

export function mergeLiteralSearchResults(
  branchResults: SymbolCandidate[][],
  limit: number,
): SymbolCandidate[] {
  if (limit === 0) return [];

  const merged: SymbolCandidate[] = [];
  const seen = new Set<string>();
  const cursors = branchResults.map(() => 0);

  while (merged.length < limit) {
    let added = false;

    for (let branchIndex = 0; branchIndex < branchResults.length; branchIndex += 1) {
      const candidates = branchResults[branchIndex] ?? [];

      while ((cursors[branchIndex] ?? 0) < candidates.length) {
        const candidateIndex = cursors[branchIndex] ?? 0;
        const candidate = candidates[candidateIndex];
        cursors[branchIndex] = candidateIndex + 1;
        if (!candidate || seen.has(candidate.qualifiedName)) continue;

        seen.add(candidate.qualifiedName);
        merged.push(candidate);
        added = true;
        break;
      }

      if (merged.length === limit) return merged;
    }

    if (!added) return merged;
  }

  return merged;
}

export function isFtsSyntaxError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /fts5|unknown special query|syntax error/i.test(message);
}

function compileBranch(input: string): LiteralSearchBranch | undefined {
  if (input.length === 0) return undefined;

  if (input.startsWith("*")) {
    const tokens = literalTokens(input.slice(1));
    if (tokens.length === 0) return undefined;
    return {
      kind: "namePattern",
      value: `.*${tokens.map(escapeRegex).join(".*")}$`,
    };
  }

  const tokens = literalTokens(input.replace(/^@+/, ""));
  if (tokens.length === 0) return undefined;
  return {
    kind: "query",
    value: tokens.map((token) => `"${token}"`).join(" "),
  };
}

function literalTokens(input: string): string[] {
  return input.match(literalTokenPattern) ?? [];
}

function escapeRegex(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
