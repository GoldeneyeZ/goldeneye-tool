### Task 3: Implement Ignore Precedence and Policy Matchers

<TASK-ID>GD-3</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/src/ignore_rules.rs`
- Create: `crates/goldeneye-discovery/src/policy.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Create: `crates/goldeneye-discovery/tests/ignore_parity.rs`

- [ ] **Step 1: Write failing ignore precedence tests**

```rust
#[test]
fn nested_gitignore_stacks_with_root() {
    let repo = fixture([
        (".gitignore", "root.log\n"),
        ("src/.gitignore", "generated/\n"),
        ("root.log", "x"),
        ("src/generated/x.rs", "fn x() {}"),
        ("src/main.rs", "fn main() {}"),
    ]);
    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();
    assert!(rules.is_ignored(Path::new("root.log"), false));
    assert!(rules.is_ignored(Path::new("src/generated"), true));
    assert!(!rules.is_ignored(Path::new("src/main.rs"), false));
}

#[test]
fn cbmignore_negates_global_and_builtin_skips() {
    let repo = fixture([
        (".cbmignore", "!vendor/\n!vendor/keep.rs\n"),
        ("vendor/keep.rs", "fn keep() {}"),
    ]);
    let global = write_external_ignore("vendor/\n");
    let options = DiscoveryOptions {
        global_ignore_path: Some(global),
        ..DiscoveryOptions::default()
    };
    let rules = IgnoreRules::build(repo.path(), &options).unwrap();
    assert!(rules.is_explicitly_whitelisted(Path::new("vendor"), true));
    assert!(!rules.is_ignored(Path::new("vendor/keep.rs"), false));
}
```

Also cover comments, escaped `!`/`#`, rooted patterns, directory-only patterns, `**`, last-match-wins, non-Git repositories, and nested `.cbmignore`.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery --test ignore_parity`

Expected: FAIL because ignore rules are undefined.

- [ ] **Step 3: Implement ignore engine**

Use `ignore::WalkBuilder` configured with:

```rust
let mut builder = ignore::WalkBuilder::new(root);
builder
    .hidden(false)
    .follow_links(options.follow_symlinks)
    .git_ignore(true)
    .git_exclude(true)
    .git_global(options.global_ignore_path.is_none())
    .parents(true)
    .add_custom_ignore_filename(".cbmignore");
if let Some(path) = &options.global_ignore_path {
    builder.add_ignore(path);
}
```

Build a second `CbmIgnoreIndex` by pre-scanning only `.cbmignore` files without following symlinks and adding each file through `ignore::gitignore::GitignoreBuilder`. Use `matched_path_or_any_parents` to expose whether a path is explicitly whitelisted. This whitelist check runs before built-in directory/suffix/mode policies, preserving upstream behavior where `.cbmignore` negates global, always-skip, fast-skip, and earlier custom rules.

- [ ] **Step 4: Port policy tables exactly**

`policy.rs` defines audited arrays from `src/discover/discover.c:31-108`:

- 73 always-skip directory names;
- 40 moderate/fast skip directory names;
- 31 always-ignored suffixes;
- 47 moderate/fast ignored suffixes;
- 34 moderate/fast skip filenames;
- 15 moderate/fast substring patterns;
- 29 ignored JSON filenames.

Expose:

```rust
pub fn directory_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason>;
pub fn file_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason>;
```

Policy matching remains case-sensitive. `Full` applies always lists only. `Moderate` and `Fast` apply both always and fast lists, matching upstream `cbm_should_skip_dir`.

- [ ] **Step 5: Verify ignore/policy behavior**

Run: `cargo test -p goldeneye-discovery --test ignore_parity`

Expected: all precedence and policy tests pass, including `.cbmignore` negation.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-discovery
git commit -m "[GD-3] feat: preserve discovery ignore precedence"
```
