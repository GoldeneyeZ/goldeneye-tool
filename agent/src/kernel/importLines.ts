// How a file brings in a symbol it uses: the import, use or re-export statement
// that names it. Read from the text, so it holds for a file changed since
// indexing and for every language that names what it imports.

/** A line that starts an import, a use or a re-export statement. */
const STATEMENT_START =
  /^\s*(?:from\s+\S+\s+import\b|import\b|export\s+(?:type\s+)?[{*]|(?:pub(?:\([^)]*\))?\s+)?use\b|(?:const|let|var)\s[^=]*=\s*require\s*\()/;

/** One name of a list written over several lines: `name,`, `name as other,`, `type Name,`. */
const LIST_ITEM = /^\s*(?:type\s+)?[\w$]+(?:\s+as\s+[\w$]+)?\s*,?\s*$/;

/** How far above a list item its statement may start. */
const STATEMENT_MAX_LINES = 60;

export interface ImportStatement {
  /** The statement as written, without its indentation; a list item follows the line that opens its list. */
  spelling: string;
  /** Written inside a function or a block rather than at the top of the file. */
  nested: boolean;
}

/**
 * The import statement that names `name` on line `index` (0-based) of a file,
 * or undefined when that line names it for another reason.
 */
export function importStatementAt(fileLines: string[], index: number, name: string): ImportStatement | undefined {
  const line = fileLines[index];
  if (line === undefined || !wordOf(name).test(line)) return undefined;
  if (STATEMENT_START.test(line)) return { spelling: line.trim(), nested: /^\s/.test(line) };
  if (!LIST_ITEM.test(line)) return undefined;

  const first = Math.max(0, index - STATEMENT_MAX_LINES);
  for (let above = index - 1; above >= first; above -= 1) {
    const text = fileLines[above];
    if (STATEMENT_START.test(text)) {
      return opensList(text) ? { spelling: `${text.trim()} … ${line.trim()}`, nested: /^\s/.test(text) } : undefined;
    }
    if (text.trim().length > 0 && !LIST_ITEM.test(text)) return undefined;
  }
  return undefined;
}

/** A statement whose list of names is still open at the end of its first line. */
function opensList(line: string): boolean {
  const opened = (line.match(/[({]/g) ?? []).length;
  const closed = (line.match(/[)}]/g) ?? []).length;
  return opened > closed;
}

function wordOf(name: string): RegExp {
  return new RegExp(`(?<![\\w$])${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![\\w$])`);
}
