# GD-7 Spec Review

- Result: checked
- Reviewer: task-worker self-audit after isolated reviewer timed out without producing artifacts
- Reviewed range: `c7a8f41..working-tree`

## Evidence Reviewed

- Task 7 requirements and final integration findings: 2 Critical, 3 Important.
- Pinned upstream `discover.c:514-610,653-675`.
- Full working-tree diff for discovery source, regressions, and frozen manifest.
- RED evidence recorded in `context.md` for every defect class.
- Fresh focused evidence: discovery crate 49/49; upstream parity 4/4; junction/root-containment regression 1/1; clippy and format exit 0.

## Compliance Notes

- Project root Git, nested Git, and `.git/info/exclude` are terminal; `.cbmignore` negation clears only a global-ignore candidate.
- `.git`, `node_modules`, `.worktrees`, and `.claude-worktrees` are checked before whitelist recovery.
- File suffix, filename, and fast-pattern policies run before ignore matching in every mode.
- `follow_symlinks` is absent from public options and walker branches; POSIX links and Windows junction/reparse entries are skipped without target reads.
- Recursive ignore-file discovery is absent. Per-directory `.gitignore`/`.cbmignore` matchers load lazily into a bounded cache only for visited/query scopes; excluded safety trees are not opened.
- Frozen manifest adds pinned-source rows for precedence conflicts, all four safety directories, all four negated file policies, and outside-root file/directory links.
- No unrelated production behavior was added.
