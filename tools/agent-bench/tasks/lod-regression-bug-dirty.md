One test fails after my local changes and I cannot see why.

The working tree contains my uncommitted work in progress. It is a clean-up
that was not supposed to change any behaviour. With it,
`tests/datasets/test_expansion_combine.py` fails; on `HEAD` it passes. The
other tests I ran still pass.

```
$ PYTHONPATH=src python -m unittest discover -s tests/datasets -p test_expansion_combine.py
E
======================================================================
ERROR: test_combined_portable_sources_reject_duplicate_targets (test_expansion_combine.CombineTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "tests/datasets/test_expansion_combine.py", line 17, in test_combined_portable_sources_reject_duplicate_targets
    result = combine_manifests([root / "first/manifest.json", root / "second/manifest.json"],
  File "src/lod/datasets/expansion/combine.py", line 46, in combine_manifests
    raise ValueError("Duplicate ID or target across combined datasets")
ValueError: Duplicate ID or target across combined datasets

----------------------------------------------------------------------
Ran 1 test in 0.072s

FAILED (errors=1)
```

Find the cause and fix it.

- Fix it where it goes wrong, not where it shows. After the fix the code must
  behave exactly as it does on `HEAD`: the same results and the same errors
  for the same inputs. Loosening a check until this test passes is not a fix.
- Keep the rest of my work in progress. Do not revert it, neither as a whole
  nor file by file, and do not rewrite it: change only what the fix needs.
- Do not change the test.

Do not run the test suite; a held-out check runs after your response. You may
run quick source and diff checks only.
