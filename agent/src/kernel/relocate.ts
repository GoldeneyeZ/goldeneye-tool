// Finds a symbol's current line range in a file that changed after indexing.
// The index only says where the symbol used to be; this re-locates its
// declaration by text and measures its body structurally. Heuristic by design:
// callers must label the result as located by text.

export interface LineRange {
  startLine: number;
  endLine: number;
}

export interface LocateOptions {
  /** The symbol is a type; a same-named constructor must not win. */
  type?: boolean;
  /** 1-based index among same-named declarations (overloads). */
  occurrence?: number;
}

type DeclarationForm = "type" | "member";

const TYPE_KEYWORDS =
  "class|interface|struct|enum|trait|type|record|union|protocol|object|namespace|module|extension";
const MEMBER_KEYWORDS = "fn|def|function|func|const|static|let|var|val";
const MAX_BODY_LINES = 600;

/** @param hintLine 1-based line where the symbol was when indexed */
export function locateDeclaration(
  lines: string[],
  name: string,
  hintLine: number,
  options: LocateOptions = {},
): LineRange | undefined {
  const typeForms: number[] = [];
  const memberForms: number[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const form = declarationForm(lines, index, name);
    if (form === "type") typeForms.push(index);
    else if (form === "member") memberForms.push(index);
  }

  // A type and its constructors share a name; keep the kind that was asked for.
  const [preferred, other] = options.type ? [typeForms, memberForms] : [memberForms, typeForms];
  const candidates = preferred.length > 0 ? preferred : other;
  if (candidates.length === 0) return undefined;

  const occurrence = options.occurrence;
  const chosen =
    occurrence !== undefined && occurrence <= candidates.length
      ? candidates[occurrence - 1]
      : candidates.reduce((best, index) =>
          Math.abs(index + 1 - hintLine) < Math.abs(best + 1 - hintLine) ? index : best,
        );

  let start = chosen;
  while (start > 0 && /^\s*(@|#\[|#!\[)/.test(lines[start - 1])) start -= 1;
  return { startLine: start + 1, endLine: bodyEnd(lines, chosen) + 1 };
}

function declarationForm(lines: string[], index: number, name: string): DeclarationForm | undefined {
  const line = lines[index];
  const escaped = escapeRegex(name);
  if (new RegExp("^\\s*#\\s*define\\s+" + escaped + "\\b").test(line)) return "member";
  if (/^\s*(\/\/|\/\*|\*|#(?!\[)|--)/.test(line)) return undefined;

  const afterKeyword = (keywords: string) =>
    new RegExp("\\b(" + keywords + ")\\s+(\\w+\\s+)*" + escaped + "\\b").test(line);
  if (afterKeyword(TYPE_KEYWORDS)) return "type";
  if (afterKeyword(MEMBER_KEYWORDS)) return "member";

  // `name(` is a declaration when a type or modifier precedes it, or when the
  // statement opens a block before it ends; otherwise it is a call.
  const match = new RegExp("(^|[^.\\w:])" + escaped + "\\s*(<[^>()]*>)?\\s*\\(").exec(line);
  if (match === null) return undefined;
  const before = line.slice(0, match.index + match[1].length).trim();
  if (/(\breturn|\bnew|\bthrow|\bawait|[=(,!&|?:+\-*/])$/.test(before)) return undefined;
  if (/[\w>\]*&]$/.test(before)) return "member";
  return terminator(lines, index, match.index) === "{" ? "member" : undefined;
}

/** First `{` or `;` outside parentheses, scanning forward from a position. */
function terminator(lines: string[], lineIndex: number, column: number): "{" | ";" | undefined {
  let depth = 0;
  for (let index = lineIndex; index < Math.min(lines.length, lineIndex + 12); index += 1) {
    const text = stripLiterals(lines[index]).slice(index === lineIndex ? column : 0);
    for (const character of text) {
      if (character === "(" || character === "[") depth += 1;
      else if (character === ")" || character === "]") depth -= 1;
      else if (depth === 0 && (character === "{" || character === ";")) return character;
    }
  }
  return undefined;
}

function bodyEnd(lines: string[], declaration: number): number {
  const limit = Math.min(lines.length, declaration + MAX_BODY_LINES);

  // A macro ends with its last continued line.
  if (/^\s*#\s*define\b/.test(lines[declaration])) {
    let end = declaration;
    while (end + 1 < limit && /\\\s*$/.test(lines[end])) end += 1;
    return end;
  }

  // Indentation-delimited body (Python and similar).
  if (/:\s*(#.*)?$/.test(lines[declaration]) && !lines[declaration].includes("{")) {
    const indent = indentation(lines[declaration]);
    let end = declaration;
    for (let index = declaration + 1; index < limit; index += 1) {
      if (lines[index].trim().length === 0) continue;
      if (indentation(lines[index]) <= indent) break;
      end = index;
    }
    return end;
  }

  let parens = 0;
  let braces = 0;
  let opened = false;
  for (let index = declaration; index < limit; index += 1) {
    for (const character of stripLiterals(lines[index])) {
      if (character === "(" || character === "[") parens += 1;
      else if (character === ")" || character === "]") parens -= 1;
      else if (character === "{") {
        braces += 1;
        opened = true;
      } else if (character === "}") {
        braces -= 1;
        if (opened && braces === 0) return index;
      } else if (character === ";" && !opened && parens <= 0) {
        return index;
      }
    }
  }
  return declaration;
}

function indentation(line: string): number {
  return line.length - line.trimStart().length;
}

/** Blanks out string, character and comment content so braces in them are ignored. */
function stripLiterals(line: string): string {
  return line
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, "``")
    .replace(/\/\/.*$/, "")
    .replace(/\/\*.*?\*\//g, "");
}

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
