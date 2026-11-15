Remove Vim mode from the code editor.

We are dropping the editor's Vim emulation. It is off by default, very few people turn it on, its `:q` closes a tab without going through the unsaved-changes guard, and the plugin behind it is one more dependency to keep up to date. Remove the feature completely; do not hide it behind a flag.

Requirements:

1. The editor no longer has a Vim mode: no Vim key bindings, no `:w` / `:q` / `:wq` / `:x` commands, and no code path that can switch it on.
2. The setting is gone everywhere: the preference itself (its type, storage key, default, loading, setter and change notification) and its row on the settings page. A value already saved under that key in a user's settings file is left where it is and never read again; do not add migration or clean-up code for it.
3. Nothing that existed only for Vim mode is left behind. That covers modules, exports, styling and plumbing: a prop, ref or callback whose only caller was Vim mode is removed at every layer it passes through, not kept as dead code.
4. Everything else works exactly as before. Whatever Vim mode shared with other features stays: the other editor settings, the other editor extensions and key bindings, saving files, closing tabs. Code elsewhere in the application that merely mentions Vim for another purpose is not part of this feature; leave it alone.
5. Remove the npm package that only Vim mode used from `package.json`. Do not edit `pnpm-lock.yaml`; it is regenerated with `pnpm install` afterwards.
6. The README must no longer list Vim mode as a feature. Leave `ROADMAP.md` as it is; it records what was shipped.

Do not modify Rust files or existing tests. No new tests are required.

The project must still type-check (`tsc --noEmit`, which also rejects unused imports and declarations) and the existing frontend tests must still pass.
