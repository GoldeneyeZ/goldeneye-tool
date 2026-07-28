# Plan Progression

Last updated: 2026-07-28 00:00

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
- Next action: Implement next pending task.

## Task 1: Add failing isolation contract

- Path: `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation/tasks/ABFI-1/`
- Status: pending
- Next action: Start implementation.

## Task 2: Relocate benchmark entrypoints

- Path: `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation/tasks/ABFI-2/`
- Status: pending
- Next action: Start implementation.

## Task 3: Add private package boundary and operator README

- Path: `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation/tasks/ABFI-3/`
- Status: pending
- Next action: Start implementation.

## Task 4: Migrate every tracked entrypoint reference

- Path: `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation/tasks/ABFI-4/`
- Status: pending
- Next action: Start implementation.

## Task 5: Verify production separation and full acceptance

- Path: `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation/tasks/ABFI-5/`
- Status: pending
- Next action: Start implementation.