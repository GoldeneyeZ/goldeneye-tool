# GD-3 Spec Review

Status: **PASS**

Reviewed commit: `5efa1cb593c64f7ebd75340ed39f33b7af99ced7`

## Requirement Audit

- `IgnoreRules` configures `ignore::WalkBuilder` with hidden files enabled, option-controlled symlink following, Git ignore/exclude/global sources, parent rules, `.cbmignore`, and optional external global ignore.
- Separate `CbmIgnoreIndex` pre-scans `.cbmignore` files without following symlinks, builds each scoped matcher through `GitignoreBuilder`, and evaluates with `matched_path_or_any_parents`.
- Custom match result is evaluated before standard ignore sources; explicit custom whitelists therefore negate external/global rules and expose the policy override signal required by GD-4.
- Root and nested `.gitignore`/`.cbmignore` precedence is last-match-wins, with deeper scoped matchers evaluated later.
- Tests cover comments, escaped `!`/`#`, rooted rules, directory-only rules, `**`, last-match-wins, nested custom rules, non-Git repositories, external global negation, and WalkBuilder recovery.
- All seven policy tables match ordered upstream data exactly: 73/40/31/47/34/15/29.
- Directory and file matchers are case-sensitive. `Full` applies always policy only; `Moderate` and `Fast` add fast policy.
- Public exports match task API: `IgnoreRules`, `directory_policy`, and `file_policy`.

No missing, extra, or misunderstood requirement found.
