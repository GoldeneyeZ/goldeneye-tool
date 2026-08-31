export function normalizeFilePattern(pattern: string | undefined): string | undefined {
  if (pattern === undefined || !/[*?]/.test(pattern)) return pattern;

  // Preserve patterns that clearly use regular-expression syntax. Otherwise,
  // accept the common glob spelling agents naturally use for file filters.
  if (pattern.includes(".*") || /[\\^$()[\]{}|+]/.test(pattern)) return pattern;

  let normalized = "^";
  for (let index = 0; index < pattern.length; index += 1) {
    const char = pattern[index];
    if (char === "*" && pattern[index + 1] === "*") {
      if (pattern[index + 2] === "/") {
        normalized += "(?:.*/)?";
        index += 2;
      } else {
        normalized += ".*";
        index += 1;
      }
    } else if (char === "*") {
      normalized += "[^/]*";
    } else if (char === "?") {
      normalized += "[^/]";
    } else {
      normalized += /[.\\]/.test(char) ? `\\${char}` : char;
    }
  }
  return `${normalized}$`;
}
