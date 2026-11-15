#!/usr/bin/env python3
"""Held-out checker for the `lod-regression-bug-dirty` task.

    python lod-regression-bug-dirty.check.py <worktree> [<source repo>]

Exit 0 = pass; failures are printed, one per line. Standard library only,
Python >= 3.9, offline, a few seconds.

The task: uncommitted work in progress (the overlay) refactors the synthetic
expansion generators under `src/lod/datasets/expansion/`. One call in it drops
the seed from the namespace that `opaque.generate_rows` hashes its labels and
call IDs under, so two datasets built with different seeds contain the same
opaque targets and `tests/datasets/test_expansion_combine.py` fails in
`combine.py`. The agent has to find and fix that, keep the rest of the work,
and leave the test alone.

What is checked:

1. The failing test file is byte-for-byte the one of the base commit.
2. The rest of the work in progress is still there (parsed with `ast`): the
   new `naming.py` and its helpers, their use in `opaque.py` and
   `dense_labels.py`, `_payload_seed` in `varied.py`, `_character_bucket` in
   `build.py`, the renamed locals in `combine.py` and `dense_labels.py`. No
   marker sits on the line that carries the regression, so a fix at the call
   site, in the helper's signature or by inlining that one expression passes.
3. Behaviour is the one of the base commit. The base sources are read with
   `git archive` into a temporary directory and the same probe runs against
   both trees: generated rows of the three generators for several seeds and
   splits, built manifests, a combined corpus and its loaded rows, and the
   rejection of a repeated target that arrives under new row IDs. This fails
   a symptom patch (the duplicate check weakened, call IDs made unique while
   labels still ignore the seed) as well as the unfixed tree.
4. The base commit's copies of the expansion tests pass against the worktree
   sources, run from the temporary directory (22 tests; the one test that
   needs tiktoken's downloaded vocabulary is left out).
"""

import ast
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import warnings

BASE = "1c6a727d20aeccabe1da2dbb6d4a5b16a4afe6a4"
FAILING_TEST = "tests/datasets/test_expansion_combine.py"
PACKAGE = "src/lod/datasets/expansion"

# file -> (functions it must define, names it must still use)
KEPT_WORK = {
    "naming.py": (("scoped", "uuid7_like"), ("DEFAULT_SEED", "SPLITS")),
    "opaque.py": ((), ("uuid7_like", "scoped", "DEFAULT_SEED", "SPLITS")),
    "dense_labels.py": ((), ("uuid7_like", "scoped", "DEFAULT_SEED", "target_raw")),
    "varied.py": (("_payload_seed",), ("_payload_seed", "DEFAULT_SEED")),
    "build.py": (("_character_bucket",), ("_character_bucket", "DEFAULT_SEED", "characters")),
    "combine.py": ((), ("copied_rows",)),
}

HELD_OUT_TESTS = [
    "test_expansion_build",
    "test_expansion_combine",
    "test_expansion_loading",
    "test_expansion_opaque",
    "test_expansion_token_audit",
    "test_expansion_varied",
    "test_expansion_dense_labels.DenseLabelTests.test_fitlong_has_real_structure_and_disjoint_labels",
    "test_expansion_dense_labels.DenseLabelTests.test_complete_dense_labels_formats_and_trace",
    "test_expansion_dense_labels.DenseLabelTests.test_determinism_and_disjoint_splits",
]
HELD_OUT_TEST_COUNT = 22

PROBE = r'''
import gzip
import hashlib
import json
import sys
import tempfile
from pathlib import Path

SPLITS = ("train", "validation", "test")
out = {}


def sha(value):
    text = json.dumps(value, sort_keys=True, ensure_ascii=False)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def record(key, thunk):
    try:
        out[key] = thunk()
    except Exception as error:  # reported and compared, never fatal
        out[key] = "raised %s: %s" % (type(error).__name__, error)


def renamed_copy(source, target):
    """The same targets as `source` under new row IDs, as a valid dataset."""
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    target.mkdir()
    for shard in manifest["shards"]:
        with gzip.open(source / shard["path"], "rt", encoding="utf-8") as stream:
            rows = [json.loads(line) for line in stream]
        for row in rows:
            row["id"] += "-copy"
            row["episode_id"] += "-copy"
        path = target / shard["path"]
        with path.open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as stream:
                for row in rows:
                    stream.write((json.dumps(row, ensure_ascii=False) + "\n").encode("utf-8"))
        shard["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        shard["bytes"] = path.stat().st_size
    (target / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")


def main():
    from lod.datasets.expansion import dense_labels, opaque, varied
    from lod.datasets.expansion.build import build_dataset
    from lod.datasets.expansion.combine import combine_manifests
    from lod.datasets.expansion.loading import load_expanded

    for split in SPLITS:
        for seed in (1, 2, 17, 20260910):
            record("opaque.generate_rows(52, %r, %d)" % (split, seed),
                   lambda: sha(list(opaque.generate_rows(52, split, seed))))
        for seed in (1, 2, 20260910):
            record("varied.generate_rows(40, %r, %d)" % (split, seed),
                   lambda: sha(list(varied.generate_rows(40, split, seed))))
        for seed in (1, 20260910):
            record("dense_labels.generate_rows(3, %r, %d)" % (split, seed),
                   lambda: sha(list(dense_labels.generate_rows(3, split, seed))))
        record("dense_labels.generate_rows(2, %r, 7, tier='fitlong')" % split,
               lambda: sha(list(dense_labels.generate_rows(2, split, 7, tier="fitlong"))))
    record("opaque.generate_rows(8) with default arguments",
           lambda: sha(list(opaque.generate_rows(8))))
    record("varied.generate_rows(8) with default arguments",
           lambda: sha(list(varied.generate_rows(8))))
    record("dense_labels.generate_rows(2) with default arguments",
           lambda: sha(list(dense_labels.generate_rows(2))))

    def labels(seed):
        return {label for split in SPLITS for row in opaque.generate_rows(16, split, seed)
                for label in row["required_labels"]}

    record("number of opaque labels shared by seed 1 and seed 2",
           lambda: len(labels(1) & labels(2)))

    counts = {split: {"varied": 3, "opaque": 4} for split in SPLITS}
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for name, options in (("seed1", {"seed": 1}), ("seed2", {"seed": 2}), ("default", {})):
            record("build_dataset manifest, %s" % name,
                   lambda: sha(build_dataset(root / name, counts, shard_rows=5, **options)))
        record("combine_manifests of the seed 1 and seed 2 datasets",
               lambda: sha(combine_manifests([root / "seed1/manifest.json",
                                              root / "seed2/manifest.json"], root / "combined")))
        record("rows loaded from the combined corpus",
               lambda: sha(list(load_expanded(root / "combined/manifest.json"))))
        record("combine_manifests of one dataset given twice",
               lambda: sha(combine_manifests([root / "seed1/manifest.json"] * 2, root / "twice")))
        record("combine_manifests of a dataset and a copy of its targets under new row IDs",
               lambda: (renamed_copy(root / "seed1", root / "renamed"),
                        sha(combine_manifests([root / "seed1/manifest.json",
                                               root / "renamed/manifest.json"],
                                              root / "repeated")))[1])


try:
    main()
except Exception as error:
    out["probe"] = "raised %s: %s" % (type(error).__name__, error)
sys.stdout.write(json.dumps(out))
'''


def main():
    if len(sys.argv) < 2:
        print("usage: lod-regression-bug-dirty.check.py <worktree> [<source repo>]")
        return 2
    root = os.path.abspath(sys.argv[1])
    source = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2] else None
    if not os.path.isdir(os.path.join(root, *PACKAGE.split("/"))):
        print("not a LOD worktree (no %s): %s" % (PACKAGE, root))
        return 1
    temporary = tempfile.mkdtemp(prefix="lod-regression-bug-")
    try:
        failures = grade(root, source, temporary)
    finally:
        shutil.rmtree(temporary, ignore_errors=True)
    if failures:
        for index, failure in enumerate(failures, 1):
            print("%d. %s" % (index, failure))
        return 1
    print("PASS: the seed reaches the opaque labels again, generated data matches the base commit, "
          "the work in progress is kept, the test is untouched, %d expansion tests pass"
          % HELD_OUT_TEST_COUNT)
    return 0


def grade(root, source, temporary):
    base = os.path.join(temporary, "base")
    error = extract_base(base, [root] + ([source] if source else []))
    if error:
        return [error]
    failures = []
    failures.extend(check_test_untouched(root, base))
    failures.extend(check_kept_work(root))
    failures.extend(check_behaviour(root, base))
    failures.extend(run_held_out_tests(root, base, os.path.join(temporary, "heldout")))
    return failures


def extract_base(target, repositories):
    """Base sources and tests, read from the object database, never from the worktree."""
    problems = []
    for repository in repositories:
        try:
            result = subprocess.run(
                ["git", "-C", repository, "archive", "--format=tar", BASE, "src/lod", "tests/datasets"],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
        except (OSError, subprocess.TimeoutExpired) as error:
            problems.append("%s: %s" % (repository, error))
            continue
        if result.returncode != 0:
            problems.append("%s: %s" % (repository, result.stderr.decode("utf-8", "replace").strip()[-200:]))
            continue
        os.makedirs(target, exist_ok=True)
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)  # extraction filter, 3.12+
            with tarfile.open(fileobj=io.BytesIO(result.stdout), mode="r:") as archive:
                archive.extractall(target)
        return None
    return "cannot read base commit %s with git archive (%s)" % (BASE, " | ".join(problems))


def normalized(path):
    with open(path, "rb") as stream:
        return stream.read().replace(b"\r\n", b"\n")


def check_test_untouched(root, base):
    path = os.path.join(root, *FAILING_TEST.split("/"))
    if not os.path.isfile(path):
        return ["%s was deleted; the test must stay as it is" % FAILING_TEST]
    if normalized(path) != normalized(os.path.join(base, *FAILING_TEST.split("/"))):
        return ["%s was modified; the test must stay as it is" % FAILING_TEST]
    return []


def check_kept_work(root):
    failures = []
    for name, (functions, names) in KEPT_WORK.items():
        relative = "%s/%s" % (PACKAGE, name)
        path = os.path.join(root, *relative.split("/"))
        if not os.path.isfile(path):
            failures.append("work in progress lost: %s is missing" % relative)
            continue
        try:
            with open(path, encoding="utf-8") as stream:
                tree = ast.parse(stream.read(), filename=relative)
        except (SyntaxError, UnicodeDecodeError) as error:
            failures.append("%s does not parse: %s" % (relative, error))
            continue
        defined = {node.name for node in tree.body if isinstance(node, ast.FunctionDef)}
        used = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Name):
                used.add(node.id)
            elif isinstance(node, ast.Attribute):
                used.add(node.attr)
            elif isinstance(node, ast.arg):
                used.add(node.arg)
        missing = ["def %s" % function for function in functions if function not in defined]
        missing += [wanted for wanted in names if wanted not in used and wanted not in defined]
        if missing:
            failures.append("work in progress lost in %s: no longer has %s"
                            % (relative, ", ".join(missing)))
    return failures


def run_probe(tree):
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", PYTHONIOENCODING="utf-8",
                       PYTHONPATH=os.path.join(tree, "src"))
    try:
        result = subprocess.run([sys.executable, "-c", PROBE], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, env=environment, cwd=tree, timeout=120)
    except subprocess.TimeoutExpired:
        return None, "timed out"
    if result.returncode != 0:
        tail = result.stderr.decode("utf-8", "replace").strip().splitlines()[-3:]
        return None, " | ".join(tail) or "exit %d" % result.returncode
    try:
        return json.loads(result.stdout.decode("utf-8")), None
    except ValueError:
        return None, "unreadable output"


def check_behaviour(root, base):
    expected, error = run_probe(base)
    if error or "probe" in expected:
        return ["grader problem: the behaviour probe failed on the base commit: %s"
                % (error or expected["probe"])]
    actual, error = run_probe(root)
    if error:
        return ["the behaviour probe could not run against the worktree: %s" % error]
    if "probe" in actual:
        return ["the expansion package no longer imports or runs: %s" % actual["probe"]]
    failures = []
    for key, wanted in expected.items():
        got = actual.get(key)
        if got == wanted:
            continue
        if isinstance(got, str) and got.startswith("raised "):
            detail = got
        elif isinstance(wanted, str) and wanted.startswith("raised "):
            detail = "did not fail, the base commit %s" % wanted
        elif isinstance(wanted, int) and not isinstance(wanted, bool):
            detail = "is %r, %r on the base commit" % (got, wanted)
        else:
            detail = "differs from the base commit"
        failures.append("behaviour changed: %s: %s" % (key, detail))
    if len(failures) > 12:
        failures = failures[:12] + ["... and %d more behaviour differences" % (len(failures) - 12)]
    return failures


def run_held_out_tests(root, base, directory):
    os.makedirs(directory)
    tests = os.path.join(base, "tests", "datasets")
    for name in os.listdir(tests):
        if name.startswith("test_expansion_") and name.endswith(".py"):
            shutil.copyfile(os.path.join(tests, name), os.path.join(directory, name))
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", PYTHONIOENCODING="utf-8",
                       PYTHONPATH=os.path.join(root, "src"))
    try:
        result = subprocess.run([sys.executable, "-m", "unittest", *HELD_OUT_TESTS],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                env=environment, cwd=directory, timeout=150)
    except subprocess.TimeoutExpired:
        return ["the expansion tests of the base commit timed out"]
    text = result.stdout.decode("utf-8", "replace")
    counted = re.search(r"^Ran (\d+) tests? in", text, re.M)
    ran = int(counted.group(1)) if counted else 0
    if result.returncode == 0 and ran >= HELD_OUT_TEST_COUNT:
        return []
    interesting = [line.strip() for line in text.splitlines()
                   if re.match(r"(ERROR|FAIL): |\w*(Error|Exception)\b|FAILED|Ran \d+", line.strip())]
    return ["the expansion tests of the base commit do not pass against the worktree (%d run): %s"
            % (ran, " | ".join(interesting[:10]) or text.strip()[-400:])]


if __name__ == "__main__":
    sys.exit(main())
