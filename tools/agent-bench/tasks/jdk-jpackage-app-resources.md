# Add jpackage `--app-resources`

Implement a repeatable `--app-resources <additional-resources>` option for
jpackage application-image creation.

Requirements:

- Accept one or more existing files/directories, separated by the platform path
  separator (`:` on Linux/macOS, `;` on Windows). Support repeated option
  occurrences using normal jpackage option-array semantics.
- Reject the option outside application-image-building scopes.
- Copy selected content into the platform application resources directory:
  Windows application-image root, Linux `lib`, macOS
  `Contents/Resources`.
- Preserve nested paths and normal rooted-path collision semantics. Repeated
  values must keep the same last-option precedence convention as
  `--app-content`.
- Resource copying happens after input payload copying but before
  `--app-content`; therefore colliding `--app-content` files always win,
  independent of command-line option order.
- Carry resource sources through CLI parsing, `ApplicationBuilder`, immutable
  `Application`, layout override/copy operations, and image-copy actions.
- Extend the application-layout model/mixin/builder with a non-null resources
  directory. Update every Linux/macOS layout construction site.
- Include the option in help formatting, localized help resources, and
  `jpackage.md`, including destination and precedence documentation.
- Update/add focused jpackage unit/integration tests: parsing/help, layout/model
  construction, copy behavior, repeated values, platform separator behavior,
  and fill-order collisions.
- Preserve existing `--app-content` behavior and project style.

Keep changes inside `src/jdk.jpackage/**` and
`test/jdk/tools/jpackage/**`. Do not alter build infrastructure.
