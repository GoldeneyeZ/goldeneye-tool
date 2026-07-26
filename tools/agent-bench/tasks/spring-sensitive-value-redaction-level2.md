# Spring sensitive-value redaction (Level 2)

Implement opt-in sensitive-value redaction across Spring binding and method
validation.

## Required public contracts

- Add runtime, documented, meta-annotatable
  `org.springframework.core.annotation.Sensitive`. It must support fields,
  bean accessor methods, method parameters, record components, and annotation
  types. Composed annotations must be detected.
- Add the following Spring validation extension contracts:
  `SensitiveValueContext`, `SensitiveValueDetector`, and
  `SensitiveValueRedactor`. A detector decides whether a value is sensitive
  from immutable context; a redactor receives that context and the rejected
  value and returns the value representation retained in public errors.
  Context must expose the root target type, object name when available,
  canonical property path or method parameter, and whether the failure is a
  binding failure.
- `DataBinder` must allow detector and redactor configuration with
  `setSensitiveValueDetector(...)` and `setSensitiveValueRedactor(...)`.
- `ConfigurableWebBindingInitializer` must expose the same configuration and
  copy it to every created `WebDataBinder`.

## Behavior

The defaults are annotation-based detection and replacement with
`[REDACTED]`. Values without an applicable marker must remain unchanged. Do
not use property-name heuristics.

Apply redaction to rejected-value representations for bean and direct-field
access, conversion failures, validator rejection, constructor binding, and
nested or indexed paths. Annotation lookup must work through fields,
accessors, record components, and composed annotations.

For method validation, redact only the argument representation in
`ParameterValidationResult` for a sensitive parameter. The actual invocation
arguments must not change. Redaction must also propagate through Spring MVC
and WebFlux binding and validation; no web layer may restore the original
value in an error representation or diagnostic exception text.

Redaction is representation-only. Never mutate a target, raw request/property
value, successfully converted value, or invocation argument. Preserve message
codes, error arguments, default messages, binding-failure flags, field names,
container indexes and keys, source-object unwrapping, constraint violations,
and return-value validation behavior.

## Validation and scope

Add focused production tests and run the relevant module tests. Do not run
`clean`. Do not change build scripts, dependency declarations, generated
files, or files outside `spring-core`, `spring-beans`, `spring-context`,
`spring-web`, `spring-webmvc`, and `spring-webflux`.
