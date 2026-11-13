import { describe, expect, it } from "vitest";
import { importStatementAt } from "../src/kernel/importLines.js";

const at = (lines: string[], index: number, name: string) => importStatementAt(lines, index, name);

describe("import statements", () => {
  it("reads a statement written on one line, in each language that names what it imports", () => {
    const statements = [
      "from lod.training.pairs import canonical",
      "from lod.training.pairs import load_pairs, canonical as compact",
      'import { canonical, other } from "./pairs";',
      'import type { canonical } from "./pairs";',
      'export { canonical } from "./pairs";',
      "import static com.shop.Json.canonical;",
      "use crate::json::{canonical, strict};",
      "pub(crate) use json::canonical;",
      'const { canonical } = require("./pairs");',
    ];
    for (const statement of statements) {
      expect(at([statement], 0, "canonical")).toEqual({ spelling: statement, nested: false });
    }
  });

  it("says when the statement is inside a function", () => {
    const lines = ["def run():", "    from lod.training.pairs import canonical", "    return canonical(1)"];
    expect(at(lines, 1, "canonical")).toEqual({ spelling: "from lod.training.pairs import canonical", nested: true });
  });

  it("reads a name of a list written over several lines", () => {
    const python = ["from lod.training.pairs import (", "    load_pairs,", "    canonical,", ")"];
    expect(at(python, 2, "canonical")).toEqual({
      spelling: "from lod.training.pairs import ( … canonical,",
      nested: false,
    });
    const typescript = ["import {", "  type Pair,", "  canonical as compact,", '} from "./pairs";'];
    expect(at(typescript, 2, "canonical")?.spelling).toBe("import { … canonical as compact,");
  });

  it("leaves out every other line that names the symbol", () => {
    const lines = [
      "def canonical(value):",
      "    return canonical(value)",
      "# import canonical from the pairs module",
      "export function canonical(value) {}",
      "important = canonical",
      "names = [",
      "    canonical,",
      "]",
      "from lod.training.pairs import canonical_event_json",
    ];
    for (const [index] of lines.entries()) expect(at(lines, index, "canonical")).toBeUndefined();
  });

  it("does not take a list item below a statement that is already closed", () => {
    const lines = ["from a import (b, c)", "canonical,"];
    expect(at(lines, 1, "canonical")).toBeUndefined();
  });
});
