const WINDOWS_ABSOLUTE = /^[A-Za-z]:[\\/]/;

export function normalizeRepoPath(value) {
  const normalized = String(value).replaceAll("\\", "/").replace(/^\.\/+/, "");
  if (
    !normalized ||
    normalized.startsWith("/") ||
    WINDOWS_ABSOLUTE.test(normalized) ||
    normalized.split("/").includes("..")
  ) {
    throw new Error(`Dirty path must be repository-relative: ${value}`);
  }
  return normalized;
}

export function compileDirtyPathPolicy(config = {}) {
  const normalizePrefix = (value) => {
    const normalized = normalizeRepoPath(value);
    return normalized.endsWith("/") ? normalized : `${normalized}/`;
  };

  return {
    exact: new Set((config.exact ?? []).map(normalizeRepoPath)),
    prefixes: (config.prefixes ?? []).map(normalizePrefix),
    globs: (config.globs ?? []).map((value) => {
      const source = normalizeRepoPath(value);
      return { source, regex: globToRegExp(source) };
    }),
    min_paths: config.min_paths ?? 0,
    max_paths: config.max_paths ?? Infinity,
    required_prefixes: (config.required_prefixes ?? []).map(normalizePrefix),
  };
}

export function evaluateDirtyPaths(paths, policy) {
  const normalized = [...new Set(paths.map(normalizeRepoPath))].sort();
  const allowed = (path) =>
    policy.exact.has(path) ||
    policy.prefixes.some((prefix) => path.startsWith(prefix)) ||
    policy.globs.some((glob) => glob.regex.test(path));
  const disallowed = normalized.filter((path) => !allowed(path));
  const missingRequiredPrefixes = policy.required_prefixes.filter(
    (prefix) => !normalized.some((path) => path.startsWith(prefix)),
  );
  const belowMinimum = normalized.length < policy.min_paths;
  const aboveMaximum = normalized.length > policy.max_paths;

  return {
    passed:
      disallowed.length === 0 &&
      missingRequiredPrefixes.length === 0 &&
      !belowMinimum &&
      !aboveMaximum,
    normalized,
    disallowed,
    missing_required_prefixes: missingRequiredPrefixes,
    below_minimum: belowMinimum,
    above_maximum: aboveMaximum,
  };
}

function globToRegExp(glob) {
  let source = "";
  for (let index = 0; index < glob.length; index += 1) {
    const char = glob[index];
    if (char === "*" && glob[index + 1] === "*") {
      source += ".*";
      index += 1;
    } else if (char === "*") {
      source += "[^/]*";
    } else if (char === "?") {
      source += "[^/]";
    } else {
      source += char.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
    }
  }
  return new RegExp(`^${source}$`);
}
