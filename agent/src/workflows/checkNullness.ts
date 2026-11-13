import type { CodeContext } from "./readCode.js";

// A Java package that is non-null by default (JSpecify's @NullMarked, Spring's
// @NonNullApi) is built with a nullness checker: a method whose return type has
// no @Nullable may not return a value that can be null. The source parses, the
// tests of the change pass in the head of whoever wrote it, and the build
// fails. One case can be told from the text alone, and is the one reported:
// a `return` of `null`, or of a parameter or local declared @Nullable, in a
// method the change wrote.

const JAVA_FILE = /\.java$/;
const NULL_MARKED = /@(NullMarked|NonNullApi|NonNullByDefault)\b/;
const FILES_CHECKED = 60;
const ROWS_SHOWN = 8;
const HEADER_TAIL_MAX_CHARS = 400;
const NOT_A_METHOD = new Set([
  "if",
  "for",
  "while",
  "switch",
  "catch",
  "synchronized",
  "try",
  "return",
  "new",
  "super",
  "this",
  "throw",
  "else",
  "do",
  "assert",
  "yield",
  "case",
]);
const NOT_A_TYPE = new Set([...NOT_A_METHOD, "class", "interface", "enum", "record", "package", "import", "instanceof"]);
const MODIFIER = /\b(?:public|protected|private|static|final|abstract|synchronized|native|default|strictfp)\b/g;
// `Void` has no value but null.
const PRIMITIVE = new Set(["void", "Void", "boolean", "byte", "short", "int", "long", "char", "float", "double"]);

export interface NullableReturn {
  /** 1-based line of the `return`. */
  line: number;
  method: string;
  /** `null`, or the name of the @Nullable parameter or local that is returned. */
  value: string;
}

function blank(text: string): string {
  return text.replace(/[^\n]/g, " ");
}

/** Index of the bracket that closes the one at `open`, or -1. */
function closing(code: string, open: number): number {
  const opener = code[open];
  const closer = opener === "(" ? ")" : "}";
  let depth = 0;
  for (let index = open; index < code.length; index += 1) {
    if (code[index] === opener) depth += 1;
    else if (code[index] === closer) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

/**
 * The source with comments, literals and the arguments of annotations replaced
 * by spaces: every offset and line stays where it was, and what is left of a
 * bracket or a keyword is code.
 */
function codeOnly(source: string): string {
  let code = source.replace(
    /\/\*[\s\S]*?\*\/|\/\/[^\n]*|"""[\s\S]*?"""|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'/g,
    blank,
  );
  const annotation = /@[\w.]+[ \t]*\(/g;
  for (let match = annotation.exec(code); match !== null; match = annotation.exec(code)) {
    const open = match.index + match[0].length - 1;
    const close = closing(code, open);
    if (close < 0) break;
    code = code.slice(0, open) + blank(code.slice(open, close + 1)) + code.slice(close + 1);
    annotation.lastIndex = close + 1;
  }
  return code;
}

/** `text` split at the commas that are inside no bracket. */
function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if ("([<{".includes(character)) depth += 1;
    else if (")]>}".includes(character)) depth -= 1;
    else if (character === "," && depth === 0) {
      parts.push(text.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim()).filter((part) => part !== "");
}

function withoutOuterParentheses(expression: string): string {
  let text = expression.trim();
  while (text.startsWith("(") && closing(text, 0) === text.length - 1) text = text.slice(1, -1).trim();
  return text;
}

/** The three parts of `a ? b : c` when the expression is one at its top level. */
function conditional(expression: string): [string, string, string] | undefined {
  let depth = 0;
  let question = -1;
  let nested = 0;
  for (let index = 0; index < expression.length; index += 1) {
    const character = expression[index];
    if ("([{".includes(character)) depth += 1;
    else if (")]}".includes(character)) depth -= 1;
    else if (depth !== 0) continue;
    else if (character === "?") {
      if (question < 0) question = index;
      else nested += 1;
    } else if (character === ":" && question >= 0) {
      // `::` is a method reference, not the else of a conditional.
      if (expression[index + 1] === ":" || expression[index - 1] === ":") continue;
      if (nested > 0) nested -= 1;
      else return [expression.slice(0, question), expression.slice(question + 1, index), expression.slice(index + 1)];
    }
  }
  return undefined;
}

/** The value in `expression` that can be null as written: `null`, or a name of `nullable`. */
function nullableValue(expression: string, nullable: Set<string>): string | undefined {
  const text = withoutOuterParentheses(expression);
  if (text === "null") return "null";
  if (/^[\w$]+$/.test(text)) return nullable.has(text) ? text : undefined;
  const parts = conditional(text);
  if (parts === undefined) return undefined;
  for (const branch of [parts[1], parts[2]]) {
    const value = nullableValue(branch, nullable);
    if (value === undefined) continue;
    // `value != null ? value : other` has tested it.
    if (value !== "null" && new RegExp(`\\b${value}\\b`).test(parts[0])) continue;
    return value;
  }
  return undefined;
}

/** Whether the method tests `name` for null, or assigns it, anywhere: then nothing certain can be said. */
function isGuarded(body: string, variable: string): boolean {
  const name = variable.replace(/\$/g, "\\$");
  return new RegExp(
    [
      `\\b${name}\\s*[!=]=\\s*null\\b`,
      `\\bnull\\s*[!=]=\\s*${name}\\b`,
      `\\b${name}\\s+instanceof\\b`,
      `\\b${name}\\s*=[^=]`,
      `\\b(?:requireNonNull|notNull|nonNull|isNull|state|isTrue|hasText|hasLength|isEmpty|isInstanceOf)\\s*\\(\\s*${name}\\b`,
    ].join("|"),
  ).test(body);
}

/**
 * Returns of a value that can be null from a method whose return type has no
 * @Nullable. `isAdded` says whether a 0-based line was written by the change;
 * only a method whose header or `return` is one of those is reported.
 */
export function nullableReturns(source: string, isAdded: (line: number) => boolean): NullableReturn[] {
  const code = codeOnly(source);
  const lineStarts = [0];
  for (let index = 0; index < code.length; index += 1) if (code[index] === "\n") lineStarts.push(index + 1);
  const lineAt = (offset: number): number => {
    let low = 0;
    let high = lineStarts.length - 1;
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if (lineStarts[middle] <= offset) low = middle;
      else high = middle - 1;
    }
    return low;
  };

  const found: NullableReturn[] = [];
  const declaration = /\b([A-Za-z_$][\w$]*)\s*\(/g;
  for (let match = declaration.exec(code); match !== null; match = declaration.exec(code)) {
    const name = match[1];
    if (NOT_A_METHOD.has(name)) continue;
    const open = match.index + match[0].length - 1;
    const close = closing(code, open);
    if (close < 0) continue;
    const tail = /^\s*(?:throws\s+[\w.,\s]+?)?\s*\{/.exec(code.slice(close + 1, close + 1 + HEADER_TAIL_MAX_CHARS));
    if (tail === null) continue;
    const bodyOpen = close + tail[0].length;
    const bodyClose = closing(code, bodyOpen);
    if (bodyClose < 0) continue;

    let start = match.index;
    while (start > 0 && !";{}".includes(code[start - 1])) start -= 1;
    const prefix = code.slice(start, match.index);
    const returnType = prefix.replace(/@[\w.]+/g, " ").replace(MODIFIER, " ").trim();
    // A constructor has no type; a generic method returns what its caller chose.
    // (An enum constant with a body also reads as `NAME(...) {`: it starts with no type.)
    if (!/^[A-Za-z_$][\w\s.<>,[\]?&$]*$/.test(returnType)) continue;
    if (returnType.split(/[^\w$]+/).some((word) => NOT_A_TYPE.has(word))) continue;
    if (PRIMITIVE.has(returnType) || /@(?:Nullable|Contract)\b/.test(prefix)) continue;
    // `@SuppressWarnings("NullAway")`: the argument is in the source, not in the code read here.
    if (/@SuppressWarnings\b/.test(prefix) && /null/i.test(source.slice(start, match.index))) continue;

    // What a lambda, an anonymous class or a local class returns is not what the method returns.
    let body = code.slice(bodyOpen + 1, bodyClose);
    const inner =
      /(?:->\s*|\bnew\s+[\w.$<>,?\s[\]]+\([^;{}]*\)\s*|\b(?:class|interface|enum|record)\s+[\w$]+[^;{}]*)\{/g;
    for (let block = inner.exec(body); block !== null; block = inner.exec(body)) {
      const innerOpen = block.index + block[0].length - 1;
      const innerClose = closing(body, innerOpen);
      if (innerClose < 0) break;
      body = body.slice(0, innerOpen + 1) + blank(body.slice(innerOpen + 1, innerClose)) + body.slice(innerClose);
      inner.lastIndex = innerClose;
    }

    const nullable = new Set<string>();
    for (const parameter of splitTopLevel(code.slice(open + 1, close))) {
      const parameterName = /([\w$]+)\s*(?:\[\s*\]\s*)*$/.exec(parameter)?.[1];
      if (parameterName !== undefined && /@Nullable\b/.test(parameter)) nullable.add(parameterName);
    }
    for (const local of body.matchAll(/@Nullable\s+(?:final\s+)?[\w.$<>,?[\]\s]+?\s+([\w$]+)\s*[=;]/g)) {
      nullable.add(local[1]);
    }
    // A local is assigned where it is declared: what counts for it is a second assignment.
    for (const candidate of [...nullable]) {
      const declared = new RegExp(`@Nullable\\s+(?:final\\s+)?[\\w.$<>,?[\\]\\s]+?\\s+${candidate}\\s*=`, "g");
      if (isGuarded(body.replace(declared, (text) => blank(text)), candidate)) nullable.delete(candidate);
    }

    let headerAdded = false;
    for (let line = lineAt(match.index); line <= lineAt(bodyOpen); line += 1) headerAdded ||= isAdded(line);
    for (const statement of body.matchAll(/\breturn\b([^;]*);/g)) {
      const line = lineAt(bodyOpen + 1 + statement.index);
      if (!headerAdded && !isAdded(line)) continue;
      const value = nullableValue(statement[1], nullable);
      if (value !== undefined) found.push({ line: line + 1, method: name, value });
    }
  }
  return found;
}

/** The annotation that makes the package of `filePath` non-null by default, if one does. */
async function nullMarker(context: CodeContext, filePath: string, source: string): Promise<string | undefined> {
  const directory = filePath.includes("/") ? filePath.slice(0, filePath.lastIndexOf("/") + 1) : "";
  const packageInfo = await context.reader.lines(`${directory}package-info.java`);
  return NULL_MARKED.exec((packageInfo ?? []).join("\n"))?.[0] ?? NULL_MARKED.exec(codeOnly(source))?.[0];
}

/**
 * In the Java files that differ from HEAD and whose package is non-null by
 * default: the returns of a nullable value from a method without @Nullable on
 * its return type, among the methods the change wrote.
 */
export async function nullableLeaks(context: CodeContext): Promise<string[]> {
  if (context.newFiles === undefined) return [];
  const changed = (await context.newFiles.changed()).filter((filePath) => JAVA_FILE.test(filePath));
  const rows: string[] = [];
  const markers = new Set<string>();
  for (const filePath of changed.slice(0, FILES_CHECKED)) {
    const lines = await context.reader.lines(filePath);
    if (lines === undefined) continue;
    const source = lines.join("\n");
    const marker = await nullMarker(context, filePath, source);
    if (marker === undefined) continue;
    const before = new Set(((await context.newFiles.atHead(filePath)) ?? "").split(/\r?\n/).map((line) => line.trim()));
    const isAdded = (line: number): boolean => {
      const text = (lines[line] ?? "").trim();
      return text !== "" && !before.has(text);
    };
    for (const item of nullableReturns(source, isAdded)) {
      markers.add(marker);
      const what = item.value === "null" ? "null" : `${item.value}, which is @Nullable`;
      rows.push(`  ${filePath}:${item.line}\t${item.method} returns ${what}; its return type has no @Nullable`);
    }
  }
  if (rows.length === 0) return [];
  const more = rows.length > ROWS_SHOWN ? [`  ... ${rows.length - ROWS_SHOWN} more`] : [];
  return [
    `# nullness: the package is non-null by default (${[...markers].join(", ")}); a build that checks it rejects these:`,
    ...rows.slice(0, ROWS_SHOWN),
    ...more,
  ];
}
