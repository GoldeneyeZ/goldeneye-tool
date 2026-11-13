import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { nullableLeaks, nullableReturns } from "../src/workflows/checkNullness.js";
import type { CodeContext } from "../src/workflows/readCode.js";

let workspace: string | undefined;

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

async function createContext(files: Record<string, string>, head: Record<string, string>): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-null-"));
  for (const [filePath, text] of Object.entries(files)) {
    await mkdir(join(workspace, dirname(filePath)), { recursive: true });
    await writeFile(join(workspace, filePath), text);
  }
  const client = { tool: async () => ({}) };
  return {
    client,
    project: "demo",
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: {
      all: async () => Object.keys(files),
      list: async () => Object.keys(files).filter((filePath) => head[filePath] === undefined),
      changed: async () => Object.keys(files).filter((filePath) => files[filePath] !== head[filePath]),
      deleted: async () => [],
      atHead: async (filePath: string) => head[filePath],
    } as unknown as NewFiles,
  };
}

const everyLine = (): boolean => true;

function java(...lines: string[]): string {
  return lines.join("\n");
}

describe("nullableReturns", () => {
  it("names a return of a @Nullable parameter from a method whose return type is not @Nullable", () => {
    const source = java(
      "class Result {",
      "  final Object rejected(String field, @Nullable Object value) {",
      "    return (isSensitive(field) ? REDACTED : value);",
      "  }",
      "  Object direct(@Nullable Object value) {",
      "    return value;",
      "  }",
      "  String none() {",
      "    return null;",
      "  }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([
      { line: 3, method: "rejected", value: "value" },
      { line: 6, method: "direct", value: "value" },
      { line: 9, method: "none", value: "null" },
    ]);
  });

  it("says nothing where the return type is @Nullable, generic, primitive or absent", () => {
    const source = java(
      "class Result {",
      "  @Nullable Object annotated(@Nullable Object value) { return value; }",
      "  @Nullable",
      "  public Object above(@Nullable Object value) {",
      "    return value;",
      "  }",
      "  Object @Nullable [] array(Object @Nullable [] value) { return value; }",
      "  <T> T chosen(@Nullable T value) { return value; }",
      "  Void nothing() { return null; }",
      "  boolean flag(@Nullable Object value) { return value == null; }",
      "  Result(@Nullable Object value) { this.value = value; return; }",
      '  @SuppressWarnings("NullAway")',
      "  Object suppressed() { return null; }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([]);
  });

  it("says nothing of a value the method has tested or assigned", () => {
    const source = java(
      "class Result {",
      "  Object tested(@Nullable Object value) {",
      "    if (value == null) {",
      "      return EMPTY;",
      "    }",
      "    return value;",
      "  }",
      "  Object conditional(@Nullable Object value) {",
      "    return (value != null ? value : EMPTY);",
      "  }",
      "  Object asserted(@Nullable Object value) {",
      '    Assert.notNull(value, "no value");',
      "    return value;",
      "  }",
      "  Object replaced(@Nullable Object value) {",
      "    value = fallback();",
      "    return value;",
      "  }",
      "  Object local() {",
      "    @Nullable Object found = lookup();",
      "    found = (found != null ? found : EMPTY);",
      "    return found;",
      "  }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([]);
  });

  it("names a @Nullable local that is returned as it was declared", () => {
    const source = java(
      "class Result {",
      "  Object local() {",
      "    @Nullable Object found = lookup();",
      "    return found;",
      "  }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([{ line: 4, method: "local", value: "found" }]);
  });

  it("leaves to a lambda, an anonymous class and a local class what they return", () => {
    const source = java(
      "class Result {",
      "  Supplier<Object> lazy() {",
      "    return () -> {",
      "      return null;",
      "    };",
      "  }",
      "  Callable<Object> task() {",
      "    return new Callable<>() {",
      "      @Override",
      "      public @Nullable Object call() {",
      "        return null;",
      "      }",
      "    };",
      "  }",
      "  Object holder() {",
      "    class Holder {",
      "      @Nullable Object get() { return null; }",
      "    }",
      "    return new Holder();",
      "  }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([]);
  });

  it("reads no code in comments, strings and annotation arguments, and no method in a statement", () => {
    const source = java(
      "enum Kind {",
      "  FIRST(1) {",
      "    @Override @Nullable Object value() { return null; }",
      "  },",
      "  SECOND(2) {",
      "    @Override @Nullable Object value() { return null; }",
      "  };",
      "  // Object commented() { return null; }",
      '  static final String TEXT = "Object quoted() { return null; }";',
      '  @SuppressWarnings({"unchecked", "rawtypes"})',
      "  Object real(@Nullable Object value) {",
      "    if (check(value)) {",
      "      synchronized (this) {",
      "        for (Object item : items(value)) {",
      "          use(item);",
      "        }",
      "      }",
      "    }",
      "    /* return null; */",
      "    return value;",
      "  }",
      "}",
    );

    expect(nullableReturns(source, everyLine)).toEqual([{ line: 20, method: "real", value: "value" }]);
  });

  it("reports only a method whose header or return the change wrote", () => {
    const source = java(
      "class Result {",
      "  Object old(@Nullable Object value) {",
      "    return value;",
      "  }",
      "  Object changed(@Nullable Object value) {",
      "    return (flag ? value : EMPTY);",
      "  }",
      "}",
    );

    expect(nullableReturns(source, (line) => line === 5)).toEqual([{ line: 6, method: "changed", value: "value" }]);
  });
});

describe("nullableLeaks", () => {
  const result = java(
    "package demo.validation;",
    "",
    "import org.jspecify.annotations.Nullable;",
    "",
    "public class Result {",
    "  final Object rejected(String field, @Nullable Object value) {",
    "    return (isSensitive(field) ? REDACTED : value);",
    "  }",
    "}",
  );
  const head = java("package demo.validation;", "", "public class Result {", "}");

  it("reports a nullable return in a package that is non-null by default", async () => {
    const context = await createContext(
      {
        "src/demo/validation/package-info.java": "@NullMarked\npackage demo.validation;\n",
        "src/demo/validation/Result.java": result,
      },
      {
        "src/demo/validation/package-info.java": "@NullMarked\npackage demo.validation;\n",
        "src/demo/validation/Result.java": head,
      },
    );

    expect(await nullableLeaks(context)).toEqual([
      "# nullness: the package is non-null by default (@NullMarked); a build that checks it rejects these:",
      "  src/demo/validation/Result.java:7\trejected returns value, which is @Nullable; its return type has no @Nullable",
    ]);
  });

  it("says nothing in a package that has no such default", async () => {
    const context = await createContext(
      { "src/demo/validation/Result.java": result },
      { "src/demo/validation/Result.java": head },
    );

    expect(await nullableLeaks(context)).toEqual([]);
  });
});
