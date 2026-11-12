Write a developer document for the indexing pipeline of this repository: the
code that turns a repository checkout into the code graph stored in SQLite.
There is no such document yet.

Create `docs/architecture/indexing-pipeline.md` with these sections, in this order:

1. **Purpose** - what the pipeline produces, its entry point, what starts a
   run (name the callers) and how concurrent runs are prevented.
2. **Stages of a full index** - every stage in execution order, from file
   discovery to the final write, each with the function that implements it and
   the file it lives in. The extraction part has two alternative code paths:
   document both, each with its own ordered steps, and state the condition
   that selects between them. Say which stages depend on the index mode.
3. **Key data structures** - the types that carry state between stages (the
   per-run context handed to passes, the in-memory graph, the symbol registry,
   the per-file extraction result): name, defining file, role.
4. **Persistence** - which function writes the graph to disk, which tables end
   up holding the results, and what else is recorded after the write so that
   later runs and searches work.
5. **Incremental re-index** - how a run chooses between a full and an
   incremental index, and the steps of the incremental path.
6. **Adding a new pass** - every place that has to change: declaration,
   registration in the orchestrator, build, tests.

Rules:

- Document only what the code does at this commit. Header comments and the
  README are partly out of date; where they disagree with the code, the code
  wins. Do not describe stages that are not actually called.
- Name real functions, types, tables and files with their exact identifiers.
  Do not invent names.
- Keep it factual and compact: about 100 to 200 lines of Markdown.
- Create only that file. Do not modify source code, and do not build or run
  tests.
