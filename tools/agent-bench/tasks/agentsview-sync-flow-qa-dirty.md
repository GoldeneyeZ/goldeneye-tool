I am new to this repository (Go backend under `cmd/` and `internal/`) and need
to understand its live-update path while `agentsview serve` is running: an
agent appends a few lines to a session transcript on disk (take a Claude
`.jsonl` file as the example) and shortly afterwards every open browser page
refetches its data.

Trace that path through the Go code exactly as it is in this checkout,
including any uncommitted changes in the working tree, and answer:

1. Call chain. List, in order, the functions involved from the filesystem
   notification to the server-sent event the browser receives, each with the
   file it is defined in: the watcher's event handling and its debounced flush,
   the callback that hands the changed paths to the sync engine, the engine
   entry point, every step that entry point applies to the list of changed
   paths before any file is parsed (name each function), the worker and
   per-file processing functions, the function that collects the results and
   writes them, the database call that stores an append, and how the HTTP layer
   is notified and which event name it sends on the global events stream (not
   the per-session watch stream).
2. What is dropped early. Which filesystem events does the watcher ignore?
   Which changed paths are discarded before classification, by which function
   in which file, and by what rule? What happens when no agent claims a path?
3. Unchanged, appended or rewritten. How is an unchanged file recognised and
   skipped? Which function decides that only the appended bytes need parsing,
   and what are at least two conditions that make it fall back to a full
   re-parse?
4. Failures. If parsing one file fails, does the rest of the pass continue, and
   where does the error end up? What happens to events for a browser that
   reads the stream too slowly?

Use the exact function and file names as they exist now. Do not modify any
file. Put the complete answer in the `summary` field of your final response.
