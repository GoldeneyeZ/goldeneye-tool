import { droppedImports } from "../kernel/droppedImports.js";
import type { CodeContext } from "./readCode.js";

const FILES_CHECKED = 200;
const NAMES_SHOWN = 10;
const ROWS_PER_NAME = 3;
const LINE_MAX_CHARS = 160;

function clip(text: string): string {
  const line = text.replace(/\s+$/, "");
  return line.length > LINE_MAX_CHARS ? `${line.slice(0, LINE_MAX_CHARS - 1)}…` : line;
}

/**
 * The lines for `gcal check`: in the files the change edited, the names whose
 * import the change removed while the file still uses them. Read from the
 * text of HEAD and of the working tree; the index is not needed.
 */
export async function removedImportsStillUsed(context: CodeContext): Promise<string[]> {
  if (context.newFiles === undefined) return [];
  const added = new Set(await context.newFiles.list());
  const edited = (await context.newFiles.changed()).filter((filePath) => !added.has(filePath));

  const output: string[] = [];
  let shown = 0;
  let hidden = 0;
  for (const filePath of edited.slice(0, FILES_CHECKED)) {
    const before = await context.newFiles.atHead(filePath);
    if (before === undefined) continue;
    const lines = await context.reader.lines(filePath);
    if (lines === undefined) continue;
    for (const dropped of droppedImports(filePath, before, lines.join("\n"))) {
      if (shown >= NAMES_SHOWN) {
        hidden += 1;
        continue;
      }
      shown += 1;
      output.push(
        `${filePath}: \`${clip(dropped.statement)}\` is gone, \`${dropped.name}\` is still used`,
        ...dropped.uses.slice(0, ROWS_PER_NAME).map((use) => `  ${use.line}\t${clip(use.text)}`),
      );
      if (dropped.uses.length > ROWS_PER_NAME) output.push(`  ... ${dropped.uses.length - ROWS_PER_NAME} more lines`);
    }
  }
  if (output.length === 0) return [];
  if (hidden > 0) output.push(`# ... ${hidden} more names`);
  return ["# an import the change removed, and a name the file still uses:", ...output];
}
