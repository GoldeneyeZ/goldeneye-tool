// A change that moves a function out of a file, or rewrites its imports by
// pattern, can take away an import the rest of the file still needs. The file
// parses, and nothing shows the missing name until that line runs. Comparing
// the names the import statements bind at HEAD and now finds it from the text
// alone.

type Language = "python" | "script" | "java" | "rust";

const STATEMENT_MAX_LINES = 60;
const NAME_MIN_CHARS = 2;

const STATEMENT_START: Record<Language, RegExp> = {
  python: /^\s*(?:import\s|from\s+[\w.]+\s+import\s)/,
  script: /^\s*(?:import(?=[\s{*"'])|(?:const|let|var)\s+(?:[\w$]+|\{[^=]*\})\s*=\s*(?:await\s+)?require\s*\()/,
  java: /^\s*import\s/,
  rust: /^\s*(?:pub(?:\([^)]*\))?\s+)?use\s/,
};

export interface DroppedImport {
  /** The name the removed import bound. */
  name: string;
  /** The first line of the import statement HEAD had. */
  statement: string;
  /** Lines of the file as it is now that still use the name; 1-based. */
  uses: Array<{ line: number; text: string }>;
}

function languageOf(filePath: string): Language | undefined {
  if (/\.pyi?$/i.test(filePath)) return "python";
  if (/\.(?:[cm]?[jt]s|[jt]sx)$/i.test(filePath)) return "script";
  if (/\.(?:java|kt|kts)$/i.test(filePath)) return "java";
  if (/\.rs$/i.test(filePath)) return "rust";
  return undefined;
}

/**
 * The lines with comments removed and the text of string literals blanked, so
 * that a word in prose is not read as code. Positions of what remains are kept.
 */
function codeOnly(lines: string[], language: Language): string[] {
  const masked: string[] = [];
  // The closing mark of a comment or string left open by an earlier line.
  let open: string | undefined;
  for (const line of lines) {
    let out = "";
    let index = 0;
    while (index < line.length) {
      if (open !== undefined) {
        const end = line.indexOf(open, index);
        if (end === -1) {
          out += " ".repeat(line.length - index);
          index = line.length;
        } else {
          out += " ".repeat(end + open.length - index);
          index = end + open.length;
          open = undefined;
        }
        continue;
      }
      const character = line[index];
      if (language === "python") {
        if (character === "#") break;
        if (line.startsWith('"""', index) || line.startsWith("'''", index)) {
          open = line.slice(index, index + 3);
          out += "   ";
          index += 3;
          continue;
        }
      } else {
        if (line.startsWith("//", index)) break;
        if (line.startsWith("/*", index)) {
          open = "*/";
          out += "  ";
          index += 2;
          continue;
        }
        if (character === "`" && language === "script") {
          open = "`";
          out += " ";
          index += 1;
          continue;
        }
      }
      // In Rust a single quote also starts a lifetime, so it is left alone.
      if (character === '"' || (character === "'" && language !== "rust")) {
        let end = index + 1;
        while (end < line.length && line[end] !== character) end += line[end] === "\\" ? 2 : 1;
        const closed = end < line.length;
        const stop = closed ? end : line.length;
        out += character + " ".repeat(stop - index - 1) + (closed ? character : "");
        index = closed ? end + 1 : line.length;
        continue;
      }
      out += character;
      index += 1;
    }
    masked.push(out);
  }
  return masked;
}

interface Statement {
  first: number;
  last: number;
  text: string;
}

function count(text: string, characters: RegExp): number {
  return text.match(characters)?.length ?? 0;
}

function unfinished(text: string, language: Language): boolean {
  if (count(text, /[({]/g) > count(text, /[)}]/g)) return true;
  if (language === "python") return /\\\s*$/.test(text);
  if (language === "rust") return !text.includes(";");
  // `import a, {` … `} from "m"`: complete once the module is named.
  if (language === "script") return /^\s*import\b/.test(text) && !/["'=]/.test(text);
  return false;
}

function importStatements(code: string[], language: Language): Statement[] {
  const start = STATEMENT_START[language];
  const found: Statement[] = [];
  for (let first = 0; first < code.length; first += 1) {
    if (!start.test(code[first])) continue;
    let text = code[first];
    let last = first;
    while (last - first < STATEMENT_MAX_LINES && last + 1 < code.length && unfinished(text, language)) {
      last += 1;
      text += ` ${code[last]}`;
    }
    found.push({ first, last, text });
    first = last;
  }
  return found;
}

interface Bound {
  names: string[];
  /** `import *`: any name may come from it. */
  everything: boolean;
}

function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const character of text) {
    if (character === "{") depth += 1;
    else if (character === "}") depth -= 1;
    if (character === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else current += character;
  }
  parts.push(current);
  return parts.map((part) => part.trim()).filter((part) => part.length > 0);
}

function rustNames(expression: string, bound: Bound): void {
  const text = expression.trim();
  const brace = text.indexOf("{");
  if (brace === -1) {
    const [path, alias] = text.split(/\s+as\s+/);
    const last = path.split("::").at(-1)?.trim() ?? "";
    if (alias !== undefined) {
      if (alias.trim() !== "_") bound.names.push(alias.trim());
    } else if (last === "*") bound.everything = true;
    else if (/^[\w$]+$/.test(last)) bound.names.push(last);
    return;
  }
  const owner = text.slice(0, brace).replace(/::\s*$/, "").split("::").at(-1)?.trim() ?? "";
  const close = text.lastIndexOf("}");
  for (const part of splitTopLevel(text.slice(brace + 1, close === -1 ? text.length : close))) {
    const self = /^self(?:\s+as\s+([\w$]+))?$/.exec(part);
    if (self === null) rustNames(part, bound);
    else if ((self[1] ?? owner) !== "_" && (self[1] ?? owner).length > 0) bound.names.push(self[1] ?? owner);
  }
}

/** The names one import statement makes available in its file. */
function boundBy(statement: string, language: Language): Bound {
  const bound: Bound = { names: [], everything: false };
  const item = (text: string, pick: (name: string) => string) => {
    const match = /^(?:type\s+)?([\w$.]+)(?:\s+as\s+([\w$]+))?$/.exec(text.trim());
    if (match !== null) bound.names.push(match[2] ?? pick(match[1]));
  };
  const text = statement.trim();

  if (language === "python") {
    const from = /^from\s+[\w.]+\s+import\s+([\s\S]+)$/.exec(text);
    const body = (from?.[1] ?? /^import\s+([\s\S]+)$/.exec(text)?.[1] ?? "").replace(/[()\\]/g, " ");
    for (const part of body.split(",")) {
      if (part.trim() === "*") bound.everything = true;
      // `import a.b` binds `a`; `from m import a` binds `a`.
      else item(part, (name) => (from === null ? name.split(".")[0] : name));
    }
    return bound;
  }

  if (language === "script") {
    const assigned = /^import\s+([\w$]+)\s*=/.exec(text);
    if (assigned !== null) {
      bound.names.push(assigned[1]);
      return bound;
    }
    const required = /^(?:const|let|var)\s+([\w$]+|\{[^}]*\})\s*=/.exec(text);
    if (required !== null) {
      if (!required[1].startsWith("{")) bound.names.push(required[1]);
      else {
        for (const part of required[1].slice(1, -1).split(",")) {
          const name = /([\w$]+)\s*(?:=[^,]*)?$/.exec(part.trim())?.[1];
          if (name !== undefined) bound.names.push(name);
        }
      }
      return bound;
    }
    const clause = /^import\s+(?:type\s+(?=[\w${*]))?([\s\S]*?)\s*from\s*["']/.exec(text)?.[1];
    if (clause === undefined) return bound;
    const named = /\{([\s\S]*)\}/.exec(clause);
    for (const part of named?.[1].split(",") ?? []) item(part, (name) => name);
    const rest = clause.replace(/\{[\s\S]*\}/, "");
    const namespace = /\*\s*as\s+([\w$]+)/.exec(rest);
    if (namespace !== null) bound.names.push(namespace[1]);
    const standard = /^\s*([\w$]+)/.exec(rest);
    if (standard !== null) bound.names.push(standard[1]);
    return bound;
  }

  if (language === "java") {
    const match = /^import\s+(?:static\s+)?([\w.]+?)(\.\*)?(?:\s+as\s+([\w$]+))?\s*;?$/.exec(text);
    if (match === null) return bound;
    if (match[2] !== undefined) bound.everything = true;
    else bound.names.push(match[3] ?? match[1].split(".").at(-1) ?? "");
    return bound;
  }

  rustNames(text.replace(/^(?:pub(?:\([^)]*\))?\s+)?use\s+/, "").replace(/;[\s\S]*$/, ""), bound);
  return bound;
}

interface Imports {
  /** Name to the index of the first line of the statement that binds it. */
  names: Map<string, number>;
  everything: boolean;
  /** Indexes of the lines that belong to an import statement. */
  lines: Set<number>;
}

function importsOf(code: string[], language: Language): Imports {
  const imports: Imports = { names: new Map(), everything: false, lines: new Set() };
  for (const statement of importStatements(code, language)) {
    for (let index = statement.first; index <= statement.last; index += 1) imports.lines.add(index);
    const bound = boundBy(statement.text, language);
    imports.everything ||= bound.everything;
    for (const name of bound.names) {
      if (name.length > 0 && !imports.names.has(name)) imports.names.set(name, statement.first);
    }
  }
  return imports;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** The name is given a value by the file itself: a declaration, an assignment, a parameter, a loop variable. */
function definedBy(name: string): RegExp {
  const word = escapeRegExp(name);
  return new RegExp(
    [
      `\\b(?:def|class|function|fn|struct|enum|trait|type|interface|const|let|var|static|mod|val|fun|object|record|namespace)\\s+${word}(?![\\w$])`,
      `^\\s*${word}\\s*(?::[^=]*)?=(?![=>])`,
      `\\b(?:as|for)\\s+${word}(?![\\w$])`,
      `\\b(?:def|function|fn|fun)\\b[^)]*[(,]\\s*${word}\\s*[,:)=]`,
      `(?<![\\w$.])${word}\\s*=>`,
      `[(,]\\s*${word}\\s*(?::[^,)]*)?[,)].*=>`,
      `\\blambda\\b[^:]*(?<![\\w$.])${word}(?![\\w$])[^:]*:`,
    ].join("|"),
  );
}

/**
 * The names an import bound at HEAD, that no import binds now, and that the
 * file still uses as code. Empty for a language whose imports are not read
 * (Python, JavaScript and TypeScript, Java and Kotlin, Rust are), and for a
 * file that now has an `import *`.
 */
export function droppedImports(filePath: string, before: string, after: string): DroppedImport[] {
  const language = languageOf(filePath);
  if (language === undefined) return [];
  const beforeLines = before.split(/\r?\n/);
  const afterLines = after.split(/\r?\n/);
  const had = importsOf(codeOnly(beforeLines, language), language);
  if (had.names.size === 0) return [];
  const code = codeOnly(afterLines, language);
  const has = importsOf(code, language);
  if (has.everything) return [];

  const found: DroppedImport[] = [];
  for (const [name, first] of had.names) {
    if (name.length < NAME_MIN_CHARS || has.names.has(name)) continue;
    const word = escapeRegExp(name);
    // `x.name` and `x::name` are members of something else, not this name.
    const used = new RegExp(`(?<![\\w$.])(?<!::)${word}(?![\\w$])`, "g");
    const defined = definedBy(name);
    // `f(name=1)` names a parameter; `{ name: 1 }` names a key.
    const label = language === "python" ? new RegExp(`^${word}\\s*=(?!=)`) : new RegExp(`^${word}\\s*:(?!:)`);
    const uses: DroppedImport["uses"] = [];
    let local = false;
    for (const [index, line] of code.entries()) {
      if (has.lines.has(index)) continue;
      if (defined.test(line)) {
        local = true;
        break;
      }
      for (const match of line.matchAll(used)) {
        const labels = label.test(line.slice(match.index)) && /[({,]\s*$/.test(line.slice(0, match.index));
        if (labels) continue;
        uses.push({ line: index + 1, text: afterLines[index] });
        break;
      }
    }
    if (!local && uses.length > 0) found.push({ name, statement: beforeLines[first].trim(), uses });
  }
  return found;
}
