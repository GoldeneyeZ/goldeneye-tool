import { describe, expect, it } from "vitest";
import { normalizeFilePattern } from "../src/workflows/filePattern.js";

describe("normalizeFilePattern", () => {
  it("converts common glob filters to anchored regex", () => {
    expect(normalizeFilePattern("src/jdk.jpackage/**/*.java")).toBe(
      "^src/jdk\\.jpackage/(?:.*/)?[^/]*\\.java$",
    );
    expect(normalizeFilePattern("**/*Test.java")).toBe("^(?:.*/)?[^/]*Test\\.java$");
    expect(normalizeFilePattern("**/*.properties")).toBe("^(?:.*/)?[^/]*\\.properties$");
  });

  it("preserves explicit regex and plain suffix filters", () => {
    expect(normalizeFilePattern("src/.*\\.java$")).toBe("src/.*\\.java$");
    expect(normalizeFilePattern("jpackage\\.md$")).toBe("jpackage\\.md$");
    expect(normalizeFilePattern(undefined)).toBeUndefined();
  });
});
