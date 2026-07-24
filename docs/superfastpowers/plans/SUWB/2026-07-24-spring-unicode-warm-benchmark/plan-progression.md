# SUWB Plan Progression

**Plan:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark.md`
**Package root:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark/`

## Execution Policy

- Preset: goal-driven-bypass
- Task-local gate: implementation
- Phases:
  1. implementation | scope: task | requires: none | artifact: `tasks/<TASK-ID>/context.md` | worker: `skills/goal-driven-development/implementer-prompt.md`
  2. spec-review | scope: plan | requires: all tasks implemented | artifact: `spec-review.md` | worker: `skills/goal-driven-development/spec-reviewer-prompt.md`
  3. code-quality | scope: plan | requires: spec-review checked | artifact: `code-quality.md` | worker: `skills/goal-driven-development/code-quality-reviewer-prompt.md`
  4. integration-review | scope: plan | requires: code-quality checked | artifact: `final-review.md` | worker: `skills/goal-driven-development/integration-reviewer-prompt.md`

## Goal Phases

- Implementation: pending
- Spec review: unchecked
- Code quality: unchecked
- Integration review: unchecked
- Next action: Implement SUWB-1.

## Task 1: Build immutable snapshot primitives

- Path: `tasks/SUWB-1/`
- Status: pending
- Next action: Start implementation.

## Task 2: Enforce executable timing boundary and snapshot-aware runner

- Path: `tasks/SUWB-2/`
- Status: pending
- Next action: Wait for SUWB-1 implementation.

## Task 3: Add Spring task, held-out grader, and benchmark configuration

- Path: `tasks/SUWB-3/`
- Status: pending
- Next action: Wait for SUWB-2 implementation.

## Task 4: Freeze provenance, prepare snapshot, and pass smoke gates

- Path: `tasks/SUWB-4/`
- Status: pending
- Next action: Wait for SUWB-3 implementation.

## Task 5: Run one vanilla comparison and three scored candidate repetitions

- Path: `tasks/SUWB-5/`
- Status: pending
- Next action: Wait for SUWB-4 implementation.
