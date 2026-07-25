# Spring sensitive-value redaction (Level 1)

Implement opt-in sensitive-value redaction across Spring's binding pipeline.

## Required public contracts

- Add runtime, documented `org.springframework.core.annotation.Sensitive`.
  It must support fields, bean accessor methods, method parameters, record
  components, and annotation types.
- Add the following Spring validation extension contracts:
  `SensitiveValueContext`, `SensitiveValueDetector`, and
  `SensitiveValueRedactor`. A detector decides whether a value is sensitive
  from immutable context; a redactor receives that context and the rejected
  value and returns the representation retained in public errors.
  Context must expose the root target type, object name when available,
  canonical property path, and whether the failure is a binding failure.
- `DataBinder` must allow detector and redactor configuration with
  `setSensitiveValueDetector(...)` and `setSensitiveValueRedactor(...)`.
- `ConfigurableWebBindingInitializer` must expose the same configuration and
  copy it to every created `WebDataBinder`.

## Behavior

Defaults are annotation-based detection and replacement with `[REDACTED]`.
Values without an applicable marker remain unchanged. Do not use
property-name heuristics.

Apply redaction to rejected-value representations for bean-property and
direct-field access, conversion failures, validator rejection, constructor
binding, and nested or indexed array, list, and map paths. Resolve metadata
through fields, bean accessors, and record components.

Redaction is representation-only. Never mutate a target, raw request/property
value, or successfully converted value. Preserve message codes, error
arguments, default messages, binding-failure flags, field names, container
indexes and keys, source-object unwrapping, custom binding error processors,
validators, conversion services, property editors, and message-code
resolvers.

## Validation and scope

Add focused production tests and run relevant module tests. Do not run
`clean`. Do not change build scripts, dependency declarations, generated
files, or files outside `spring-core`, `spring-beans`, `spring-context`, and
`spring-web`. Method validation, Spring MVC, and Spring WebFlux integration
are outside Level 1.
