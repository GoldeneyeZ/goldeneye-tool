Move the compact JSON serializer out of the SFT pilot module and give it an
unambiguous name.

`lod.training.pairs.canonical(value)` is used all over this repository, mostly
by code that has nothing to do with pilot pairs, and its name is easy to
confuse with `canonical_event_json` from the codec channel protocol.

Target state:

1. The function lives in `src/lod/training/evaluation.py`, next to
   `strict_json`, with exactly this signature and unchanged behaviour:

   ```python
   def canonical_json(value):
   ```

2. `lod.training.pairs` no longer defines `canonical`. The old name and the old
   location are gone for good: no alias, wrapper, re-export or deprecation shim
   may remain anywhere.
3. Update every caller. Every call, import and other reference to the old
   function, wherever it is in the repository (package code under `src/`,
   tests, standalone scripts), must use `canonical_json` and get it from
   `lod.training.evaluation`. Nothing may import `canonical_json` from
   `lod.training.pairs`. Function-local imports count too.
4. `lod.training.pairs` keeps its other functions (`load_pairs`,
   `tokenize_pair`); their imports stay as they are. `canonical_event_json` is
   a different function: leave it alone.
5. Only Python sources need to change. Do not edit dataset files, manifests,
   notebooks or historical reports, and do not add new modules.

Keep the change mechanical: no behaviour changes, no unrelated reformatting.
Do not run the test suite; the held-out grader checks the whole repository
after your response. You may run quick source and diff checks only.
