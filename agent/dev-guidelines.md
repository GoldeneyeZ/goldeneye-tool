# Development Guidelines

Use these guidelines to match my preferred backend coding style across projects. Adapt framework names to the local stack, but keep the architectural boundaries and decision rules.

## Core Style

- Prefer existing local patterns over new abstractions.
- Use domain-oriented packages and class names.
- Keep feature work separate from broad refactors.
- Follow YAGNI and KISS. Simplicity takes precedence over abstraction, but readability is the goal; when a service or method becomes complex, split it into smaller pieces and introduce design patterns or abstractions only where they clarify the code.
- Put business behavior in explicit use-case methods, not incidental helper code.
- Name methods by intent: validate, transition, snapshot, consume, count, resolve, publish.
- Add abstractions only when they remove real duplication, clarify ownership, or isolate cross-domain behavior.

## Architecture

- Favor layered flow: API/controller -> service/use-case -> repository/gateway -> model/entity.
- Prefer package-by-feature first; inside each feature package, keep layer separation clear.
- Controllers are transport adapters. They bind input, apply request validation, enforce endpoint security, call services, and return DTOs.
- Services own business rules, lifecycle guards, data composition, and transactional boundaries.
- Repositories/gateways own persistence or external-system access. Do not leak them through unrelated services.
- Models/entities represent persisted state and invariants; API DTOs represent transport contracts.
- Cross-domain workflows belong in a coordinator/query service when direct service-to-service calls become unclear.

## API and DTOs

- Do not expose persistence entities directly from API methods.
- Use request/response DTOs at API boundaries.
- Name DTOs by direction and workflow: `*CreateRequest`, `*UpdateRequest`, `*Response`, or a specific command/query name.
- Put structural validation on request DTOs.
- Put domain validation in services.
- Keep response shapes explicit and stable.
- Return explicit HTTP status codes where status matters.
- Keep controllers small enough that their behavior can be understood by reading method signatures and one service call.

## Services

- Use constructor injection and immutable dependencies.
- Prefer `private final` dependencies.
- Mark write workflows transactional when the framework supports it.
- Keep mapping from model/entity to response DTO near the service unless the project already has a mapper pattern.
- Apart from common annotation-based request validation, delegate service-layer DTO validation to a dedicated validator class when it has more than three conditions.
- Write patch updates explicitly:

```java
if (request.getName() != null) {
    entity.setName(request.getName());
}
```

- Recompute derived values server-side. Never trust client-supplied totals, statuses, ownership, or calculated fields.
- Preserve lifecycle invariants through named transition methods.
- Prefer owner-service methods with use-case intent over generic repository pass-through methods.
- Avoid service methods that only expose another layer's raw query.

## Persistence

- Keep persistence concerns behind repositories, DAOs, or gateways.
- Use clear table/entity constraints for required invariants.
- Prefer explicit indexes, foreign keys, uniqueness constraints, and checks where the database can enforce correctness.
- Use lookup/reference data for statuses and types when values are shared across code and database.
- Keep enum names and persisted lookup values aligned.
- Store money as integer minor units, not floating-point values.
- Use flexible text/blob/json fields only when the payload is intentionally schema-flexible.
- Keep read-only relationships read-only in ORM mappings when scalar foreign keys are the write source.

## Migrations

- Follow the project's existing migration workflow.
- For new tables or standard additions, add migration content in the same style and location as nearby migrations.
- For changing an existing table in early-stage schema files, prefer editing the existing migration when that is the project convention.
- Keep schema changes explicit: table shape, constraints, indexes, seed data, and rollback expectations if the project uses rollbacks.
- When adding a new status/type, update seed data plus matching application enum/constants/repositories in the same change.

## Errors

- API failures should use the project's standard API exception type.
- Let centralized advice/middleware shape HTTP error responses.
- Use status-specific errors consistently with nearby code.
- Keep error messages useful but not leaky.
- Non-API/background failures may use the exception type that best fits the execution context.

## Security

- Keep public endpoints explicit.
- Require authentication by default for protected APIs.
- Use endpoint-level authorization annotations or middleware where the framework supports them.
- Keep role naming consistent across storage, code, and authorization checks.
- Put ownership checks in reusable security expressions/policies when multiple endpoints need them.
- Do not duplicate ownership logic across controllers.

## Testing

- Add focused tests around changed service rules, lifecycle transitions, security/ownership checks, and repository queries.
- Match the nearby test style and framework.
- Use unit tests for isolated service rules.
- Use integration tests for persistence mappings, transaction behavior, security wiring, and HTTP contracts.
- For bug fixes, include a regression test that would fail without the fix.
- For docs-only changes, no build is required unless the doc claims generated output or code behavior.

## Agent Workflow

- Read the local code before proposing patterns.
- Prefer semantic IDE tools for symbol-level questions when available.
- Keep raw output small. Summarize searches, logs, builds, and large files instead of dumping them.
- Run the project-native build/test command when code behavior changes.
- Report what was verified and what was not.
- Never revert unrelated user changes.

## Avoid

- Business rules in controllers.
- API responses returning persistence entities.
- Client-supplied totals, statuses, ownership, or calculated fields.
- New status/type strings without persistence seed and code constant updates.
- Repository access from non-owner services when an owner-service method or coordinator would express the use case better.
- Pass-through methods that add no domain meaning.
- Large refactors mixed into small feature or bugfix work.
- New external integrations unless the task explicitly includes integration behavior.
