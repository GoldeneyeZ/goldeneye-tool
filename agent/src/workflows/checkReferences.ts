import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import { findByName, isContainerNode, isTypeNode, nodesInFile, textSearch, type GraphNode } from "../kernel/codeGraph.js";
import { displayName } from "../kernel/displayName.js";
import { locateDeclaration } from "../kernel/relocate.js";
import { importers } from "./importers.js";
import type { CodeContext } from "./readCode.js";

// A parse check says a file is well formed, not that the change is whole. The
// index knows what the working tree no longer has: a function that was renamed
// or removed, and who called it. A caller that still uses the old name is a
// build error that nothing shows until the build runs.

const FILES_CHECKED = 60;
const SYMBOLS_SHOWN = 10;
const ROWS_PER_SYMBOL = 8;
const LINE_MAX_CHARS = 160;
const NAME_MIN_CHARS = 3;
const NON_CALLABLE = new Set(["Variable", "Field", "Constant", "Property", "Parameter", "Import"]);

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function clip(text: string): string {
  const line = text.replace(/\s+$/, "");
  return line.length > LINE_MAX_CHARS ? `${line.slice(0, LINE_MAX_CHARS - 1)}…` : line;
}

const OWNER_SEARCH_LIMIT = 60;
const SIGNATURE_LINES = 3;

/**
 * For a method of a type: what a file that really uses it has in its text.
 * Either the name of the type, or a call of a function whose signature names
 * the type (a value of that type is then at hand without the type being
 * written). Undefined for anything that is not a method of a named type.
 */
async function relatedToOwner(context: CodeContext, node: GraphNode): Promise<RegExp | undefined> {
  const owner = displayName(node, context.project).split(".").at(-2);
  if (isTypeNode(node) || owner === undefined || !/^[A-Z]/.test(owner)) return undefined;
  const producers = new Set<string>();
  try {
    const { matches } = await textSearch(context.client, `\\b${escapeRegExp(owner)}\\b`, {
      regex: true,
      limit: OWNER_SEARCH_LIMIT,
    });
    for (const match of matches) {
      const enclosing = match.enclosing;
      if (enclosing === undefined || isTypeNode(enclosing) || NON_CALLABLE.has(enclosing.label)) continue;
      if (match.lines.some((line) => line < enclosing.startLine + SIGNATURE_LINES)) producers.add(enclosing.name);
    }
  } catch {
    // Without the search the owner's name alone decides.
  }
  const calls = [...producers].filter((name) => name.length >= NAME_MIN_CHARS && name !== node.name).map(escapeRegExp);
  const word = `(?<![\\w$])${escapeRegExp(owner)}(?![\\w$])`;
  return new RegExp(calls.length === 0 ? word : `${word}|(?<![\\w$])(?:${calls.join("|")})\\s*\\(`);
}

interface Removed {
  node: GraphNode;
  /** Declared somewhere else in the changed files: moved, not removed. */
  moved: boolean;
}

/**
 * The lines for `gcal check`: symbols the index has and the working tree no
 * longer declares, with the places the call graph says use them and that
 * still name them; and the files that still import a deleted file. Empty when
 * the change is consistent as far as the index can tell.
 */
export async function leftBehind(context: CodeContext, backend: GcalBackendClient): Promise<string[]> {
  if (context.newFiles === undefined) return [];
  const changed = await context.newFiles.changed();
  const deleted = await context.newFiles.deleted();
  const added = new Set(await context.newFiles.list());
  const touched = [...new Set([...deleted, ...changed])].filter((filePath) => !added.has(filePath)).slice(0, FILES_CHECKED);

  const texts = new Map<string, string[] | undefined>();
  const linesOf = async (filePath: string) => {
    if (!texts.has(filePath)) texts.set(filePath, await context.reader.lines(filePath));
    return texts.get(filePath);
  };

  const removed: Removed[] = [];
  for (const filePath of touched) {
    let indexed: GraphNode[];
    try {
      indexed = await nodesInFile(context.client, filePath);
    } catch {
      continue;
    }
    const lines = deleted.includes(filePath) ? undefined : await linesOf(filePath);
    // What HEAD declares is read from HEAD's text, with the same test as the
    // working tree: the index also holds symbols that are no declaration (a
    // lambda, an object member), and those cannot be "removed".
    const before = (await context.newFiles.atHead(filePath))?.split(/\r?\n/);
    if (before === undefined) continue;
    const seen = new Set<string>();
    for (const node of indexed) {
      if (isContainerNode(node) || NON_CALLABLE.has(node.label) || node.name.length < NAME_MIN_CHARS) continue;
      if (seen.has(node.name)) continue;
      seen.add(node.name);
      const options = { type: isTypeNode(node) };
      if (locateDeclaration(before, node.name, node.startLine, options) === undefined) continue;
      if (lines !== undefined && locateDeclaration(lines, node.name, node.startLine, options) !== undefined) continue;
      // Still declared in another changed file: the symbol moved.
      let moved = false;
      // `name:`, `name =` or `name(` that is not a member access: a definition, not a call on something.
      const defined = new RegExp(`(^|[^.\\w$])${escapeRegExp(node.name)}\\s*[:=(]`);
      for (const other of changed) {
        if (other === filePath) continue;
        const otherLines = await linesOf(other);
        if (otherLines === undefined) continue;
        // Declared in another changed file, or defined in some form in a file
        // that is new: that is where it went.
        if (
          locateDeclaration(otherLines, node.name, 1, options) !== undefined ||
          (added.has(other) && otherLines.some((line) => defined.test(line)))
        ) {
          moved = true;
          break;
        }
      }
      removed.push({ node, moved });
    }
  }

  const output: string[] = [];
  let shown = 0;
  for (const { node, moved } of removed) {
    if (moved || shown >= SYMBOLS_SHOWN) continue;
    let callerFiles: string[];
    try {
      const edges = await backend.callers(node.qualifiedName, { depth: 1 });
      callerFiles = [...new Set([node.filePath, ...edges.map((edge) => edge.filePath)])];
    } catch {
      continue;
    }
    const used = isTypeNode(node)
      ? new RegExp(`(?<![\\w$])${escapeRegExp(node.name)}(?![\\w$])`)
      : new RegExp(`(?<![\\w$])${escapeRegExp(node.name)}\\s*\\(`);
    const related = await relatedToOwner(context, node);
    const rows: string[] = [];
    for (const filePath of callerFiles) {
      const lines = await linesOf(filePath);
      if (lines === undefined) continue;
      // The call graph links a call to a method by its name. A file that has
      // nothing to do with the method's type calls another method of that name.
      if (related !== undefined && filePath !== node.filePath && !lines.some((line) => related.test(line))) continue;
      for (const [index, line] of lines.entries()) {
        if (!used.test(line) || /^\s*(\/\/|\/\*|\*|#(?!\[)|--)/.test(line)) continue;
        rows.push(`  ${filePath}:${index + 1}\t${clip(line)}`);
      }
    }
    if (rows.length === 0) continue;
    shown += 1;
    const more = rows.length > ROWS_PER_SYMBOL ? [`  ... ${rows.length - ROWS_PER_SYMBOL} more lines`] : [];
    output.push(
      `${displayName(node, context.project)} (${node.label}, was in ${node.filePath}) is no longer declared; its callers still name it:`,
      ...rows.slice(0, ROWS_PER_SYMBOL),
      ...more,
    );
  }
  if (output.length > 0) {
    output.unshift("# removed or renamed since the index, and still used (from the call graph):");
  }

  for (const filePath of deleted.slice(0, SYMBOLS_SHOWN)) {
    const listed = await importers(context, filePath, { limit: 20, exists: false });
    if (listed === undefined || /: 0 \(scanned \d+ files\)$/.test(listed.text)) continue;
    output.push(`# ${filePath} was deleted and is still imported:`, listed.text);
  }
  return output;
}

const JAVA_FILE = /\.java$/;
const CONSTRUCTOR_ROWS = 8;
// One index lookup per class named after `new` on an added line.
const CONSTRUCTOR_TYPES_CHECKED = 40;
const DECLARATION_MAX_LINES = 12;
// Constructors the source does not show.
const GENERATED_CONSTRUCTORS = /@(AllArgsConstructor|RequiredArgsConstructor|NoArgsConstructor|Data|Value|Builder|SuperBuilder)\b/;

/** Text with string and character literals emptied, so their commas and parentheses do not count. */
function withoutLiterals(line: string): string {
  let text = line.replace(/"(?:\\.|[^"\\])*"/g, '""').replace(/'(?:\\.|[^'\\])*'/g, "''");
  // Type arguments (`Map<String, Object>`) hold commas that separate no argument.
  for (let previous = ""; previous !== text; ) {
    previous = text;
    text = text.replace(/(?<=[\w.])<[^<>()]*>/g, "");
  }
  return text;
}

/** Number of arguments between the parenthesis at `open` and its match, or undefined when it does not close in `text`. */
function argumentCount(text: string, open: number): number | undefined {
  let depth = 0;
  let commas = 0;
  let content = false;
  for (let index = open; index < text.length; index += 1) {
    const character = text[index];
    if (character === "(" || character === "[" || character === "{") depth += 1;
    else if (character === ")" || character === "]" || character === "}") {
      depth -= 1;
      if (depth === 0) return content ? commas + 1 : 0;
    } else if (depth === 1) {
      if (character === ",") commas += 1;
      else if (!/\s/.test(character)) content = true;
    } else content = true;
  }
  return undefined;
}

interface Constructors {
  filePath: string;
  /** Parameter counts of the declared constructors, with their lines. */
  declared: Array<{ parameters: number; line: number }>;
}

/** The constructors a Java class declares in its source; undefined when they cannot be told from it. */
async function declaredConstructors(context: CodeContext, name: string): Promise<Constructors | undefined> {
  let types: GraphNode[];
  try {
    types = (await findByName(context.client, name, 8)).filter(
      (node) => isTypeNode(node) && JAVA_FILE.test(node.filePath),
    );
  } catch {
    return undefined;
  }
  // Two types of one name, or none in the index: nothing certain can be said.
  if (types.length !== 1 || types[0].label !== "Class") return undefined;
  const lines = await context.reader.lines(types[0].filePath);
  if (lines === undefined || lines.some((line) => GENERATED_CONSTRUCTORS.test(line))) return undefined;

  const declaration = new RegExp(`^\\s*(?:(?:public|protected|private)\\s+)?${escapeRegExp(name)}\\s*\\(`);
  const declared: Constructors["declared"] = [];
  for (const [index, line] of lines.entries()) {
    if (!declaration.test(line)) continue;
    const text = withoutLiterals(lines.slice(index, index + DECLARATION_MAX_LINES).join(" "));
    // A variable number of arguments matches any call.
    const end = text.indexOf(")");
    if (end < 0 || text.slice(0, end).includes("...")) return undefined;
    const parameters = argumentCount(text, text.indexOf("("));
    if (parameters === undefined) return undefined;
    declared.push({ parameters, line: index + 1 });
  }
  // No explicit constructor: the default one, or one the compiler adds.
  return declared.length === 0 ? undefined : { filePath: types[0].filePath, declared };
}

/**
 * `new Type(a, b)` on a line the change added, where the project's `Type`
 * declares constructors and none takes that many arguments. Java only: its
 * constructors are not inherited and are all in the class's own source.
 */
export async function unmatchedConstructors(context: CodeContext): Promise<string[]> {
  if (context.newFiles === undefined) return [];
  const changed = (await context.newFiles.changed()).filter((filePath) => JAVA_FILE.test(filePath));
  const known = new Map<string, Constructors | undefined>();
  const rows: string[] = [];
  for (const filePath of changed.slice(0, FILES_CHECKED)) {
    const lines = await context.reader.lines(filePath);
    if (lines === undefined) continue;
    const before = new Set(((await context.newFiles.atHead(filePath)) ?? "").split(/\r?\n/).map((line) => line.trim()));
    for (const [index, line] of lines.entries()) {
      if (before.has(line.trim()) || /^\s*(\/\/|\*|\/\*)/.test(line)) continue;
      const text = withoutLiterals(line);
      for (const match of text.matchAll(/\bnew\s+([A-Z]\w*)\s*(?:<[^<>()]*>)?\s*\(/g)) {
        const given = argumentCount(text, match.index + match[0].length - 1);
        if (given === undefined) continue;
        const name = match[1];
        if (!known.has(name)) {
          known.set(name, known.size >= CONSTRUCTOR_TYPES_CHECKED ? undefined : await declaredConstructors(context, name));
        }
        const constructors = known.get(name);
        if (constructors === undefined || constructors.declared.some((item) => item.parameters === given)) continue;
        const counts = [...new Set(constructors.declared.map((item) => item.parameters))].sort((left, right) => left - right);
        rows.push(
          `  ${filePath}:${index + 1}\tnew ${name}(...) with ${given} argument${given === 1 ? "" : "s"}; ${name} declares constructors with ${counts.join(", ")} (${constructors.filePath}:${constructors.declared.map((item) => item.line).join(",")})`,
        );
      }
    }
  }
  if (rows.length === 0) return [];
  const more = rows.length > CONSTRUCTOR_ROWS ? [`  ... ${rows.length - CONSTRUCTOR_ROWS} more`] : [];
  return ["# calls that match no declared constructor:", ...rows.slice(0, CONSTRUCTOR_ROWS), ...more];
}
