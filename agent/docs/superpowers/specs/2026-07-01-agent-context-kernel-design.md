# Goldeneye Code Agent Layer Design

## Purpose

Goldeneye Code Agent Layer (GCAL) is a local context-election tool for people who use coding agents through Codex, Claude, or similar agent environments.

GCAL helps agents choose which codebase context should enter a session. Its primary goal is to improve the value/token ratio of agent context: every included token should carry useful task signal.

The first version is not a tool for agent SDK developers. It is a workflow and tool layer for agent users working inside real codebases.

## Product Direction

GCAL starts as a hybrid product:

- a CLI/tool layer around `codebase-memory-mcp`
- a workflow kit for Codex and Claude projects
- a reusable set of context-election rules

GCAL does not initially own indexing, parsing, embedding, graph storage, or symbol extraction. Those responsibilities remain with `codebase-memory-mcp`.

GCAL owns:

- context-election workflow
- compact output contracts
- response normalization
- thresholding and trace hints
- source inclusion rules
- token-efficiency policy
- agent-facing workflow documentation

The current `abyssal-zenith` assets are the behavioral seed:

- `scripts/codebase-memory.sh`
- `skills/codebase-memory/SKILL.md`
- `AGENTS.md` rules around `codebase-memory-mcp`, semantic tools, and context-safe output

GCAL should productize those ideas rather than copy them as project-local utilities.

## Non-Goals For Phase 1

Phase 1 will not implement a full intelligent `elect` command.

An eventual command such as `gcal elect "implement booking cancellation"` is valuable, but it is complex because it needs task understanding, ranking, budgeting, and inclusion decisions. That should come later, after the lower-level primitives and workflow rules are proven.

Phase 1 also will not bundle or fork `codebase-memory-mcp`. GCAL should be designed so that bundling or tuning `codebase-memory-mcp` remains possible later if the orchestration layer proves it needs deeper control.

## Phase 1 Scope

Phase 1 provides deterministic context-election primitives that agents can compose:

```bash
gcal search <query> [--limit n] [--label label] [--file regex] [--qn regex]
gcal symbol <name-regex> [--limit n] [--label label] [--file regex] [--qn regex]
gcal inspect <query-or-qualified-name> [--limit n]
gcal get <qualified-name>
gcal callers <qualified-name> [--depth n]
gcal callees <qualified-name> [--depth n]
gcal arch
gcal status
gcal index [repo-path]
```

The agent workflow is:

1. Infer or search for a qualified symbol.
2. Inspect metadata, signature, visibility, complexity, caller counts, and callee counts.
3. Decide whether source is worth fetching.
4. Fetch exact source only when needed.
5. Trace relationships only when metadata says graph context matters.
6. Keep large or noisy output out of the conversation context.

## Output Contracts

GCAL commands are context-safe by default.

`search` and `symbol` emit compact candidate rows:

```text
qualified_name<TAB>label<TAB>file<TAB>line<TAB>signature
```

`inspect` emits compact candidate and selected-symbol metadata, but not full source:

```text
# candidates
1<TAB>Class<TAB>com.example.BookingService<TAB>src/.../BookingService.java:12<TAB>...

# selected
qualified_name=com.example.BookingService.cancelBooking
kind=Method
file=src/.../BookingService.java
line=42
lines=18
complexity=3
cognitive=4
visibility=public
signature=...
return_type=...
decorators=...
callers=4
callees=2

# inbound
...

# outbound
...
```

`get` emits source text only.

`callers` and `callees` emit one relationship path or edge per line.

`arch`, `status`, and `index` emit compact JSON.

Errors are concise and actionable. Raw MCP payloads are hidden unless debug output is explicitly enabled.

## Context Election Rules

The workflow kit and CLI behavior should encode these rules:

- Use GCAL before raw text search for code symbols.
- Prefer graph and semantic lookup for classes, methods, callers, callees, and architecture.
- Use raw text search only for string literals, configs, non-code files, or weak graph results.
- Search cheaply before fetching source.
- Inspect before source when unsure.
- Use `get` only when exact source earns its place in context.
- Prefer exact qualified names when available.
- Treat full source as expensive.
- Do not include source only because a symbol appeared in search.
- Use caller and callee traces only when they answer the current question.
- Replace large trace output with explicit follow-up commands.
- Keep noisy tool responses out of conversation context.

## Context Affordance

GCAL should document context affordance as a core principle.

A codebase has high context affordance when its names, signatures, graph relationships, module boundaries, and tests allow an agent to infer useful behavior without reading large amounts of source.

GCAL depends on cheap code signals carrying meaning:

- method and class names
- parameter names
- return types
- visibility
- annotations and decorators
- caller/callee relationships
- file and module paths
- complexity and line counts
- tests and entrypoints

Initial code quality is therefore part of the model. Small functions, explicit names, cohesive modules, and clear boundaries let GCAL include less source while preserving useful context.

Phase 1 retrieval commands stay focused on context election, not style review. They may emit soft warnings when context affordance affects retrieval quality:

```text
warning=large method; source likely needed
warning=vague symbol name; confidence reduced
warning=high caller count; use callers command rather than inline trace
```

A deeper command can be added later:

```bash
gcal affordance <qualified-name-or-file>
```

That command can diagnose naming, function size, graph ambiguity, and symbols that are expensive for agents to reason about.

## Architecture

GCAL should be structured as a small product, not a project-local shell script:

```text
goldeneye-code-agent-layer/
  src/
    cli/
    adapters/
      codebaseMemoryMcp/
    kernel/
      inspectPolicy
      tracePolicy
      affordanceSignals
    formatters/
      text
      json
  workflow/
    AGENTS.md
    skills/
      codebase-memory/
        SKILL.md
        agents/
          openai.yaml
          claude.md
  docs/
```

Core boundaries:

- CLI parses commands and flags.
- Adapter calls `codebase-memory-mcp` and hides transport details.
- Normalizer converts inconsistent MCP response shapes into GCAL-owned types.
- Kernel policy applies thresholds, trace hints, warnings, and safe defaults.
- Formatters produce compact agent-facing output.
- Workflow kit teaches Codex and Claude how to use GCAL.

The first adapter should target `codebase-memory-mcp`. Its transport can be selected during implementation, but the adapter boundary must leave room for direct MCP, gateway-based MCP, or a bundled/tuned `codebase-memory-mcp` later.

## Workflow Kit

GCAL should ship workflow assets that users can copy or install into a project:

```text
workflow/
  AGENTS.md
  skills/
    codebase-memory/
      SKILL.md
      agents/
        openai.yaml
        claude.md
```

The skill should teach this decision loop:

```text
known exact symbol -> inspect or get
uncertain symbol -> search -> inspect best candidate
need behavior body -> get exact symbol
need impact/ripple -> callers/callees after inspect
need system shape -> arch
index doubt -> status
```

The workflow kit is a first-class part of GCAL because the initial intelligence lives partly in agent behavior. The tool layer provides safe primitives; the workflow kit teaches agents how to compose them.

## Testing Strategy

Testing should focus on output contracts and context safety:

- `search` rows stay compact.
- `inspect` never emits full source.
- `get` emits source only.
- large caller counts produce follow-up commands instead of flooding output.
- low context-affordance warnings are concise.
- MCP or gateway errors become actionable GCAL errors.
- formatter output remains stable enough for agent instructions.

Adapter tests should use recorded or fixture MCP responses so policy and formatting can be tested without a live MCP server.

## Open Later Work

Later versions can add:

- `gcal elect <task>` for full context election
- token budget estimation
- ranking and confidence scoring
- project-level context policies
- deeper context-affordance diagnostics
- direct MCP server exposure of GCAL primitives
- bundled or tuned `codebase-memory-mcp`
- integration recipes for Codex, Claude, and other agent environments
