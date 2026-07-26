Make command-palette fuzzy matching accent-insensitive.

Requirements:

1. `fuzzyScore` must treat accented and unaccented forms as equivalent in both directions. Examples: `cafe` matches `Café`, `résumé` matches `resume`, and `angstrom` matches `Ångström`.
2. Support both precomposed and decomposed Unicode accents.
3. Preserve existing subsequence behavior and scoring priorities, including word-boundary, camel-case, consecutive-match, and exact-case bonuses.
4. Keep the public signatures of `fuzzyScore` and `fuzzyBest` unchanged.
5. Add focused TypeScript tests for the new behavior. Do not modify Rust files.

Run the relevant frontend tests and type checks. Keep the change focused.
