# Spring Sensitive-Value Redaction Million-Token Benchmark Design

Date: 2026-07-25
Status: approved by autonomous user direction
Owner: Goldeneye benchmark harness

## Objective

Create a reproducible Spring Framework agent benchmark whose clean vanilla lane
requires a median workload near one million cumulative input tokens while
remaining a coherent production task.

The benchmark qualifies when a clean vanilla calibration run:

- passes the held-out grader;
- consumes `800,000–1,200,000` cumulative input tokens;
- consumes at least `100,000` uncached input tokens;
- is not timed out;
- changes only task-appropriate Spring Framework files.

After qualification, freeze every input and run three fresh vanilla repetitions
and three fresh Goldeneye+GCAL repetitions in one deterministic randomized serial
matrix. The final vanilla median must satisfy the same token gates; a qualifying
pilot alone is not completion evidence.

## Why This Workload

The current Unicode truncation task is too small. Its clean vanilla median is
`81,260` input tokens and `22,636` uncached input tokens. It does not require
enough code discovery to expose a durable graph-assisted advantage.

Sensitive-value redaction is a coherent cross-module feature. Correct behavior
requires tracing:

- Java annotation and reflection metadata;
- bean-property and direct-field binding;
- conversion and property-access failures;
- validator-created field errors;
- constructor binding;
- method validation;
- shared web binding initialization;
- Spring MVC and WebFlux exception propagation.

This workload rewards structural discovery and call-path reasoning. It avoids
synthetic prompt padding and bulk search-and-replace work.

## Target Repository and Frozen Candidate

- Spring Framework source:
  `D:\Dev\IdeaProjects\spring-framework`
- Spring base commit:
  `daf955157871e4ac6f192e06b71d6cc595eb979b`
- Goldeneye candidate source:
  `D:\Dev\IdeaProjects\goldeneye-tool-baseline-nodeid`
- Initial Goldeneye candidate commit:
  `8dec5b128f3241c13aedda1d7d56286f6f3aaabe`
- Model: `gpt-5.6-terra`
- Reasoning effort: `high`

Benchmark-only harness commits may follow the initial candidate commit.
Executable Goldeneye and GCAL fingerprints must be frozen separately from
benchmark task, grader, and documentation fingerprints.

## Production Task

Implement an opt-in sensitive-value redaction pipeline for Spring binding and
method-validation error representations.

### Public marker

Add a runtime `@Sensitive` marker suitable for:

- fields;
- bean accessor methods;
- method parameters;
- record components;
- annotation types for meta-annotation use.

Marker lookup must support composed annotations.

### Detection and redaction extension points

Provide pluggable detection and redaction contracts.

Detection receives enough immutable context to decide sensitivity without
requiring consumers to inspect or log the value. Context includes:

- root target type;
- object name where available;
- canonical property path or method parameter;
- binding-failure versus validation-failure classification.

Redaction receives the same context and the rejected value and returns the
representation stored in the public error result.

Defaults:

- annotation-based detection;
- replacement string `[REDACTED]`;
- no property-name heuristics;
- no change for unmarked values.

### Data binding

`DataBinder` must expose detector and redactor configuration.

Redaction applies to rejected values stored in `FieldError` for:

- bean-property access;
- direct-field access;
- conversion/type-mismatch failures;
- `Validator.rejectValue(...)`;
- constructor binding;
- nested paths;
- indexed arrays, lists, and maps.

Metadata resolution must recognize annotations on fields, accessors, and record
components while walking nested and indexed paths.

Redaction is representation-only:

- never mutate the bound target;
- never mutate raw request/property values;
- never alter successful conversion;
- preserve field names, message codes, arguments, default messages, binding
  failure flags, and exception/source unwrapping behavior.

### Method validation

Method argument validation must redact the argument representation stored in
`ParameterValidationResult` when the corresponding parameter is sensitive.

It must preserve:

- actual invocation arguments used by validation;
- constraint violations and message resolution;
- parameter/container indexes and keys;
- source-object unwrapping;
- return-value validation behavior.

### Web initialization and propagation

`ConfigurableWebBindingInitializer` must expose detector and redactor
configuration and copy both settings to every created `WebDataBinder`.

The same configuration must work through:

- Spring MVC model-attribute binding;
- Spring MVC request-body validation;
- Spring WebFlux model-attribute binding;
- Spring WebFlux request-body validation;
- handler-method validation exceptions.

No web stack may rehydrate the original value into an error representation
after redaction.

### Compatibility

- Existing applications without `@Sensitive` remain behaviorally unchanged.
- Existing custom `BindingErrorProcessor`, validator, conversion service,
  property editors, message-code resolver, and source unwrapping continue to
  work.
- Public API follows Spring nullability, naming, documentation, and package
  conventions.
- Implementation must not add external dependencies.

## Complexity Ladder

Calibration uses a predeclared ladder. Each level has a versioned task and
grader. A level is selected only by the numeric gates below, never by
GCAL-versus-vanilla outcome.

### Level 1: Binding pipeline

Marker, detector/redactor contracts, `DataBinder`, both property-access modes,
conversion failures, validator rejection, constructor binding, nested/indexed
paths, and shared web initializer.

### Level 2: Method validation and both web stacks

Level 1 plus method-argument validation, MVC, WebFlux, and handler-validation
exception propagation.

### Level 3: Error-surface hardening

Level 2 plus:

- composed marker annotations;
- record and Kotlin-accessor-compatible metadata lookup where supported by
  existing Spring reflection facilities;
- container element paths;
- custom detector composition;
- custom context-aware redactor;
- regression checks that error formatting, equality, hashing, serialization,
  and source unwrapping cannot reveal the original value.

Start at Level 2. If a passing clean vanilla calibration run is below either
token floor, advance to Level 3. If Level 2 exceeds the upper bound, use Level
1. If Level 3 remains below the floor, expand only the predeclared Level 3
held-out cases across additional existing binding entry points; do not add
unrelated features or prompt padding.

## Held-Out Grader

The agent never sees grader source.

The grader materializes isolated tests under the temporary Spring worktree,
runs only selected suites, captures output, then removes grader-created files.

Coverage:

- annotation target, retention, and meta-annotation semantics;
- default marker replacement;
- custom detector and custom redactor;
- unmarked compatibility;
- bean-property and direct-field modes;
- type mismatch and validator rejection;
- nested and indexed paths;
- constructor-bound records;
- method parameter validation;
- MVC and WebFlux binding/validation;
- configuration propagation through `ConfigurableWebBindingInitializer`;
- preservation of target/raw values;
- preservation of message/error/source semantics;
- no original secret in public error objects, exception text, or serialized
  diagnostic output.

The grader runs module-focused Gradle tests serially with the frozen Java and
Gradle homes. Every selected test must pass.

## Harness Changes

The existing harness assumes two exact dirty paths and a fixed four-run report.
This benchmark requires:

- module-scoped dirty-path allowlists using explicit glob/prefix rules;
- rejection of changes outside:
  `spring-core`, `spring-beans`, `spring-context`, `spring-web`,
  `spring-webmvc`, and `spring-webflux`;
- required production/test-file cardinality bounds;
- variable expected lane counts in report audit;
- calibration mode that records results outside scored summaries;
- token qualification gates;
- versioned invalid calibration artifacts;
- one combined randomized `3 + 3` scored matrix;
- six-run limitations text generated from actual lane counts.

Harness tests must cover every new policy and preserve previous 3+1 report
behavior.

## GCAL Snapshot

Create one immutable warm GCAL snapshot over the six allowed Spring modules.

Snapshot gates:

- stable worktree at the pinned Spring commit;
- full Java grammar;
- canonical project root equals stable worktree path;
- no WAL, SHM, lock, or writer process at copy time;
- sorted manifest and SHA-256;
- copy-only restore before every GCAL run;
- identical manifest before and after all runs.

The vanilla lane receives no GCAL or Goldeneye tool.

## Clean-Agent Protocol

Every repetition launches:

`codex exec --ephemeral --ignore-user-config --ignore-rules`

No session is resumed or shared.

Both lanes use identical:

- model and reasoning effort;
- task prompt and response schema;
- Spring commit;
- worktree creation;
- Java/Gradle environment;
- timeout;
- held-out grader;
- allowed dirty-path policy.

Provider prefix caching cannot currently be disabled by Codex CLI. Report
cached input, uncached input, output, and reasoning tokens separately.

Use a seed derived before execution and persist the resulting six-run order.
Runs remain serial.

## Calibration Protocol

For each level:

1. freeze task and grader hashes;
2. run exactly one clean vanilla calibration;
3. require grader PASS;
4. record cumulative input, cached input, uncached input, output, wall time,
   and verified end-to-end time;
5. select the next predeclared ladder action solely from token gates.

Do not reuse a calibration run as scored evidence.

Qualification:

- `800,000 ≤ input_tokens ≤ 1,200,000`;
- `uncached_input_tokens ≥ 100,000`;
- success and grader exit `0`;
- no timeout or protocol violation.

## Scored Protocol

After qualification:

1. freeze candidate, task, grader, config, response schema, and snapshot hashes;
2. dry-run a six-run randomized matrix;
3. execute three vanilla and three warm GCAL runs serially;
4. abort on any provenance, snapshot, source, or contamination gate failure;
5. independently audit all six raw artifact directories;
6. retain invalid runs without silently replacing them.

## Report

Per run:

- correctness and exit status;
- wall, grader, completion, and verified end-to-end time;
- input, cached input, uncached input, output, reasoning, and total tokens;
- tool, command, GCAL, backend, and failed-call counts;
- discovery ordering and payload size;
- patch and dirty-path statistics;
- every frozen hash and pre-run verification result.

Per lane:

- all three raw values;
- median, range, sample standard deviation, and sample coefficient of
  variation;
- correctness count;
- no statistical-significance claim at `n = 3`.

Primary comparisons:

- correctness;
- agent wall time;
- verified end-to-end time;
- cumulative input;
- uncached input;
- uncached input plus output;
- tool and discovery turns.

## Completion Gates

Benchmark is complete only when:

- a clean vanilla calibration qualifies;
- the three-run scored vanilla median remains within `800,000–1,200,000`
  cumulative input tokens and at or above `100,000` uncached input tokens;
- six scored runs exist with three per lane;
- all six pass the held-out grader;
- independent six-run audit reports zero violations;
- candidate and Spring source remain clean at frozen commits;
- snapshot hash remains unchanged;
- no benchmark process or temporary scored worktree remains;
- final report explicitly distinguishes provider caching from GCAL snapshot
  caching;
- raw reports and the corrected user-facing analysis are retained.

## Non-Goals

- forcing one million uncached tokens;
- inflating input with generated prose or repeated files;
- measuring bulk mechanical editing;
- claiming statistical significance from three repetitions;
- upstreaming the benchmark-produced Spring patch;
- using calibration artifacts as scored evidence.
