Rename the public method `RevalidatedPath::as_path` to `checked_destination`.

`RevalidatedPath` is defined in the `goldeneye-edit` crate (module
`path_auth`). It is the value returned by `AuthorizedPath::revalidate`, and the
accessor is called from more than one crate of this workspace. The current name
says nothing about the check that produced the path; the new one should.

Requirements:

1. The method is named `checked_destination` everywhere it is referenced: its
   definition, every caller in the Rust crates under `crates/`, the tests, doc
   comments (including intra-doc links) and the Markdown documentation under
   `docs/`.
2. This is a pure rename. Keep the receiver, return type, attributes and body
   of the method as they are, and do not change behaviour at any call site.
3. Do not leave the old name behind on `RevalidatedPath` in any form: no
   forwarding method and no `#[deprecated]` alias.
4. The working tree contains uncommitted work in progress of mine. Leave that
   work in place and apply the rename to the tree as it is now, not only to
   what is committed.

Keep the change focused: no other renames, no reformatting, no unrelated edits.

Do not build the workspace and do not run the tests; a held-out check runs
after your response. You may run quick source and diff checks only.
