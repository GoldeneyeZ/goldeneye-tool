# Spring sensitive-value redaction (Level 0)

Implement basic opt-in redaction for rejected values produced by Spring
`DataBinder`.

## Contract

- Add runtime, documented `org.springframework.core.annotation.Sensitive`,
  targeting fields, bean accessor methods, and annotation types.
- Recognize composed annotations meta-annotated with `@Sensitive` using Spring
  merged-annotation semantics.
- For a field marked directly or through its getter/setter, replace the
  rejected value stored in public `FieldError` objects with `[REDACTED]`.
- Support bean-property and direct-field binding.
- For bean-property binding, resolve the marker through nested/indexed paths
  such as `credentials.password` and `accounts[0].pin`.
- Cover conversion/type-mismatch failures and `Validator.rejectValue(...)`.
- Leave unmarked rejected values unchanged. Do not use property-name
  heuristics.

Redaction is representation-only: never mutate the target, raw submitted
property value, or successfully converted value. Preserve field names,
message codes, error arguments, default messages, binding-failure flags, and
source unwrapping.

## Scope

Add focused production tests and run relevant module tests. For Gradle
verification, use one focused command with timeout 600000 ms or longer; do not
poll daemon status. Do not run `clean`. Do not change build scripts,
dependencies, generated files, or files outside `spring-core`, `spring-beans`,
and `spring-context`.

Custom detector/redactor/context extension APIs, constructor binding,
record components, web initializers, method validation, Spring MVC, and Spring
WebFlux are outside Level 0.
