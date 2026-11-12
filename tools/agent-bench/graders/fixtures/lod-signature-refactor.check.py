#!/usr/bin/env python
"""Held-out check for the `lod-signature-refactor` benchmark task.

usage: python lod-signature-refactor.check.py <worktree> [--skip-tests]

The task moves `lod.training.pairs.canonical(value)` to
`lod.training.evaluation.canonical_json(value)` and requires every caller,
import and indirect reference in the repository to follow, with no alias or
shim left behind.

The check is structural (the `ast` module over every `.py` file of the
worktree) plus a behavioural probe of the moved function and a small subset of
the repository's own unittest files that run offline with the standard library.
Call sites are counted, never located by line number: the number of old-form
call sites is measured on the base commit (through `git archive`) and the
worktree must contain at least as many new-form call sites.

Exit code 0 = pass. Failures are printed one per line on stdout.
"""

import ast
import io
import json
import os
import re
import subprocess
import sys
import tarfile

BASE_SHA = "1c6a727d20aeccabe1da2dbb6d4a5b16a4afe6a4"
OLD_NAME = "canonical"
NEW_NAME = "canonical_json"
OLD_MODULE = "lod.training.pairs"
NEW_MODULE = "lod.training.evaluation"
OLD_FILE = "src/lod/training/pairs.py"
NEW_FILE = "src/lod/training/evaluation.py"
# A different function that must survive untouched (guards against a blind
# textual replace of "canonical").
SIBLING = "canonical_event_json"
# Measured on BASE_SHA; only used when the base commit cannot be read.
FALLBACK_BASE = {"calls": 75, "call_files": 40, "sibling_refs": 7}
KEPT_IN_OLD_FILE = ("load_pairs", "tokenize_pair")
SKIP_DIRS = {"__pycache__", "node_modules", "venv", "build", "dist", "runs"}
# Repository tests that exercise the touched modules and pass on the base
# commit with only the standard library (no torch/transformers needed).
TESTS = (
    "tests/codec/test_audit_decode.py",
    "tests/codec/test_base_capability_probe.py",
    "tests/codec/test_copy_probe.py",
    "tests/codec/test_decode_probe.py",
    "tests/codec/test_format_curriculum.py",
    "tests/codec/test_literal_curriculum.py",
    "tests/codec/test_replica_capacity.py",
    "tests/codec/test_synthetic_tools.py",
    "tests/codec/test_training_evaluation.py",
    "tests/codec/test_training_pairs.py",
    "tests/datasets/test_curate_teacher_codes.py",
    "tests/datasets/test_expansion_combine.py",
    "tests/datasets/test_expansion_token_audit.py",
)
PROBE = r"""
import json, sys
sys.path.insert(0, sys.argv[1])
import lod.training.evaluation as evaluation
import lod.training.pairs as pairs
out = {}
out["text"] = evaluation.canonical_json({"b": 1, "a": [1.5, "é", None, True], "c": {"k": "v w"}})
try:
    evaluation.canonical_json(float("nan"))
    out["nan"] = "accepted"
except ValueError:
    out["nan"] = "rejected"
out["keyword"] = evaluation.canonical_json(value=[1, 2])
out["old_attribute_on_pairs"] = hasattr(pairs, "canonical")
out["old_attribute_on_evaluation"] = hasattr(evaluation, "canonical")
print(json.dumps(out))
"""
PROBE_TEXT = '{"b":1,"a":[1.5,"é",null,true],"c":{"k":"v w"}}'


def worktree_sources(root):
    """Yield (posix relative path, bytes) for every Python file in the tree."""
    for directory, names, files in os.walk(root):
        names[:] = sorted(n for n in names if n not in SKIP_DIRS and not n.startswith("."))
        for name in sorted(files):
            if name.endswith(".py"):
                path = os.path.join(directory, name)
                relative = os.path.relpath(path, root).replace(os.sep, "/")
                with open(path, "rb") as handle:
                    yield relative, handle.read()


def base_sources(root):
    """Python files of the base commit, or None when it cannot be read."""
    try:
        result = subprocess.run(
            ["git", "-C", root, "archive", "--format=tar", BASE_SHA, "--", ":(glob)**/*.py"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=60,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0 or not result.stdout:
        return None
    sources = {}
    with tarfile.open(fileobj=io.BytesIO(result.stdout)) as archive:
        for member in archive.getmembers():
            if member.isfile() and member.name.endswith(".py"):
                sources[member.name] = archive.extractfile(member).read()
    return sources or None


def dotted(node):
    parts = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if isinstance(node, ast.Name):
        parts.append(node.id)
        return ".".join(reversed(parts))
    return None


def resolve_import(relative, node):
    """Absolute module name an ImportFrom refers to."""
    if not node.level:
        return node.module or ""
    package = relative.split("/")[:-1]
    if package and package[0] == "src":
        package = package[1:]
    climb = node.level - 1
    if climb:
        package = package[: len(package) - climb] if climb <= len(package) else []
    return ".".join(package + ([node.module] if node.module else []))


class Facts(ast.NodeVisitor):
    """Collect every reference to the old and the new name in one file."""

    def __init__(self, relative):
        self.relative = relative
        self.scope = []  # enclosing function nodes
        self.old = []  # (line, description)
        self.old_calls = 0
        self.new_calls = []  # (line, "name" | "attr", scope tuple)
        self.bad_calls = []  # (line, description)
        self.new_imports = []  # (line, module, asname, scope tuple)
        self.new_defs = []  # (line, node, depth)
        self.new_rebinds = []  # line
        self.sibling_refs = 0
        self.mangled = []  # (line, identifier)
        self.toplevel_defs = set()

    def _scope(self):
        return tuple(id(node) for node in self.scope)

    def _identifier(self, name, line):
        if name == SIBLING:
            self.sibling_refs += 1
        elif name.startswith(NEW_NAME + "_") or name.startswith(OLD_NAME + "_json_"):
            self.mangled.append((line, name))

    def _function(self, node):
        if not self.scope:
            self.toplevel_defs.add(node.name)
        self._identifier(node.name, node.lineno)
        if node.name == OLD_NAME:
            self.old.append((node.lineno, "definition of `%s`" % OLD_NAME))
        elif node.name == NEW_NAME:
            self.new_defs.append((node.lineno, node, len(self.scope)))
        self.scope.append(node)
        self.generic_visit(node)
        self.scope.pop()

    visit_FunctionDef = _function
    visit_AsyncFunctionDef = _function

    def visit_ClassDef(self, node):
        if not self.scope:
            self.toplevel_defs.add(node.name)
        if node.name == OLD_NAME:
            self.old.append((node.lineno, "class named `%s`" % OLD_NAME))
        elif node.name == NEW_NAME:
            self.new_defs.append((node.lineno, node, len(self.scope)))
        self.generic_visit(node)

    def visit_ImportFrom(self, node):
        module = resolve_import(self.relative, node)
        for alias in node.names:
            self._identifier(alias.name, node.lineno)
            if alias.name == OLD_NAME:
                self.old.append((node.lineno, "import of `%s` from `%s`" % (OLD_NAME, module)))
            elif alias.asname == OLD_NAME:
                self.old.append((node.lineno, "import aliased to `%s`" % OLD_NAME))
            if alias.name == NEW_NAME:
                self.new_imports.append((node.lineno, module, alias.asname, self._scope()))
        self.generic_visit(node)

    def visit_Import(self, node):
        for alias in node.names:
            if alias.asname == OLD_NAME:
                self.old.append((node.lineno, "import aliased to `%s`" % OLD_NAME))
        self.generic_visit(node)

    def visit_Call(self, node):
        target = node.func
        name = (
            target.id
            if isinstance(target, ast.Name)
            else target.attr
            if isinstance(target, ast.Attribute)
            else None
        )
        if name == OLD_NAME:
            self.old_calls += 1
        elif name == NEW_NAME:
            kind = "name" if isinstance(target, ast.Name) else "attr"
            self.new_calls.append((node.lineno, kind, self._scope()))
            positional = len(node.args) == 1 and not node.keywords
            keyword = not node.args and [k.arg for k in node.keywords] == ["value"]
            starred = any(isinstance(a, ast.Starred) for a in node.args)
            if starred or not (positional or keyword):
                self.bad_calls.append((node.lineno, "call does not pass exactly one `value` argument"))
            if kind == "attr":
                receiver = dotted(target.value)
                if receiver and receiver.split(".")[-1] == "pairs":
                    self.bad_calls.append(
                        (node.lineno, "`%s.%s(...)` still reaches the function through the pairs module" % (receiver, NEW_NAME))
                    )
        # String based access: getattr(module, "canonical"), patch.object(module, "canonical")
        helper = name in {"getattr", "setattr", "hasattr", "delattr", "object"}
        if helper:
            for argument in node.args:
                if isinstance(argument, ast.Constant) and argument.value == OLD_NAME:
                    self.old.append((node.lineno, "string based access to `%s`" % OLD_NAME))
        self.generic_visit(node)

    def visit_Name(self, node):
        self._identifier(node.id, node.lineno)
        if node.id == OLD_NAME:
            self.old.append((node.lineno, "name `%s`" % OLD_NAME))
        elif node.id == NEW_NAME and isinstance(node.ctx, (ast.Store, ast.Del)):
            self.new_rebinds.append(node.lineno)

    def visit_Attribute(self, node):
        self._identifier(node.attr, node.lineno)
        if node.attr == OLD_NAME:
            self.old.append((node.lineno, "attribute `.%s`" % OLD_NAME))
        self.generic_visit(node)

    def visit_Constant(self, node):
        if isinstance(node.value, str) and re.fullmatch(r"[\w.]+\.%s" % OLD_NAME, node.value):
            self.old.append((node.lineno, "dotted string reference %r" % node.value))


def collect(relative, source):
    tree = ast.parse(source, filename=relative)
    compile(tree, relative, "exec", dont_inherit=True)
    facts = Facts(relative)
    facts.visit(tree)
    return facts


def measure_base(root):
    sources = base_sources(root)
    if sources is None:
        return dict(FALLBACK_BASE, origin="recorded constants")
    calls = call_files = sibling = 0
    for relative, source in sources.items():
        try:
            facts = collect(relative, source)
        except (SyntaxError, ValueError):
            continue
        calls += facts.old_calls
        call_files += 1 if facts.old_calls else 0
        sibling += facts.sibling_refs
    if not calls:
        return dict(FALLBACK_BASE, origin="recorded constants")
    return {"calls": calls, "call_files": call_files, "sibling_refs": sibling, "origin": "base commit"}


def check_signature(node):
    if not isinstance(node, ast.FunctionDef):
        return "`%s` must be a plain function" % NEW_NAME
    arguments = node.args
    problems = []
    if getattr(arguments, "posonlyargs", []) or [a.arg for a in arguments.args] != ["value"]:
        problems.append("parameters must be exactly `(value)`")
    if arguments.vararg or arguments.kwarg or arguments.kwonlyargs:
        problems.append("no *args, **kwargs or keyword-only parameters allowed")
    if arguments.defaults or arguments.kw_defaults:
        problems.append("`value` must not have a default")
    if node.decorator_list:
        problems.append("no decorators allowed")
    return "; ".join(problems)


def run_probe(root):
    source_root = os.path.join(root, "src")
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    environment.pop("PYTHONPATH", None)
    try:
        result = subprocess.run(
            [sys.executable, "-c", PROBE, source_root],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            cwd=root,
            timeout=60,
        )
    except subprocess.TimeoutExpired:
        return ["behaviour probe timed out"]
    if result.returncode != 0:
        tail = result.stderr.decode("utf-8", "replace").strip().splitlines()[-3:]
        return ["behaviour probe could not import/call %s.%s: %s" % (NEW_MODULE, NEW_NAME, " | ".join(tail))]
    out = json.loads(result.stdout.decode("utf-8"))
    failures = []
    if out["text"] != PROBE_TEXT:
        failures.append("%s changed behaviour: got %r" % (NEW_NAME, out["text"]))
    if out["nan"] != "rejected":
        failures.append("%s must keep rejecting NaN (allow_nan=False)" % NEW_NAME)
    if out["keyword"] != "[1,2]":
        failures.append("%s(value=...) must work" % NEW_NAME)
    if out["old_attribute_on_pairs"]:
        failures.append("`%s.%s` is still importable at runtime" % (OLD_MODULE, OLD_NAME))
    if out["old_attribute_on_evaluation"]:
        failures.append("`%s.%s` exists: the old name must not survive as an alias" % (NEW_MODULE, OLD_NAME))
    return failures


def run_tests(root):
    failures = []
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    environment["PYTHONPATH"] = os.path.join(root, "src")
    ran = 0
    for test in TESTS:
        path = os.path.join(root, *test.split("/"))
        if not os.path.isfile(path):
            failures.append("repository test file is missing: %s" % test)
            continue
        directory, name = os.path.split(test)
        try:
            result = subprocess.run(
                [sys.executable, "-m", "unittest", "discover", "-s", directory, "-p", name],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                env=environment,
                cwd=root,
                timeout=90,
            )
        except subprocess.TimeoutExpired:
            failures.append("repository test timed out: %s" % test)
            continue
        text = result.stdout.decode("utf-8", "replace")
        counted = re.search(r"^Ran (\d+) tests? in", text, re.M)
        ran += int(counted.group(1)) if counted else 0
        if result.returncode != 0 or not counted or int(counted.group(1)) == 0:
            interesting = [
                line.strip()
                for line in text.splitlines()
                if re.search(r"Error|^FAIL|^ERROR|FAILED", line.strip())
            ]
            failures.append("repository test failed: %s :: %s" % (test, " | ".join(interesting[-3:])[:400]))
    return failures, ran


def main(argv):
    if len(argv) < 2:
        print("usage: lod-signature-refactor.check.py <worktree> [--skip-tests]")
        return 2
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    root = os.path.abspath(argv[1])
    skip_tests = "--skip-tests" in argv
    failures = []
    limited = {}

    def fail(kind, message, limit=12):
        limited[kind] = limited.get(kind, 0) + 1
        if limited[kind] <= limit:
            failures.append(message)

    files = {}
    for relative, source in worktree_sources(root):
        try:
            files[relative] = collect(relative, source)
        except (SyntaxError, ValueError) as error:
            fail("parse", "%s does not parse: %s" % (relative, str(error)[:160]))
    if not files:
        print("no Python files found in %s" % root)
        return 1

    base = measure_base(root)

    # 1. The old form is gone everywhere.
    old_sites = 0
    old_files = 0
    for relative in sorted(files):
        facts = files[relative]
        if facts.old:
            old_files += 1
        for line, description in facts.old:
            old_sites += 1
            fail("old", "old form remains: %s:%d %s" % (relative, line, description))
    if old_sites:
        failures.append("old form `%s`: %d reference(s) left in %d file(s)" % (OLD_NAME, old_sites, old_files))

    # 2. The definition: exactly one, in the new module, with the new signature.
    definitions = [(relative, line, node, depth) for relative, facts in files.items() for line, node, depth in facts.new_defs]
    home = [d for d in definitions if d[0] == NEW_FILE and d[3] == 0]
    if NEW_FILE not in files:
        failures.append("%s is missing or does not parse" % NEW_FILE)
    elif len(home) != 1:
        failures.append("%s must define exactly one top-level `%s` (found %d)" % (NEW_FILE, NEW_NAME, len(home)))
    else:
        problem = check_signature(home[0][2])
        if problem:
            failures.append("%s:%d wrong signature for `%s`: %s" % (NEW_FILE, home[0][1], NEW_NAME, problem))
    for relative, line, _node, depth in sorted(definitions, key=lambda d: (d[0], d[1])):
        if not (relative == NEW_FILE and depth == 0):
            fail("shim", "extra definition of `%s` (wrapper/shim): %s:%d" % (NEW_NAME, relative, line))
    for relative in sorted(files):
        for line in files[relative].new_rebinds:
            fail("shim", "`%s` is rebound by assignment (alias/shim): %s:%d" % (NEW_NAME, relative, line))
    if OLD_FILE in files:
        for kept in KEPT_IN_OLD_FILE:
            if kept not in files[OLD_FILE].toplevel_defs:
                failures.append("%s must keep defining `%s`" % (OLD_FILE, kept))
    else:
        failures.append("%s is missing or does not parse" % OLD_FILE)

    # 3. Imports of the new name come from the new module only, unaliased.
    for relative in sorted(files):
        for line, module, asname, _scope in files[relative].new_imports:
            if module != NEW_MODULE:
                fail("import", "%s:%d imports `%s` from `%s`; it must come from `%s`" % (relative, line, NEW_NAME, module, NEW_MODULE))
            if asname not in (None, NEW_NAME):
                fail("import", "%s:%d imports `%s` under the alias `%s`" % (relative, line, NEW_NAME, asname))

    # 4. Every call uses the new form and resolves to an import in scope.
    total_calls = 0
    call_files = 0
    for relative in sorted(files):
        facts = files[relative]
        if facts.new_calls:
            call_files += 1
        total_calls += len(facts.new_calls)
        for line, description in facts.bad_calls:
            fail("call", "%s:%d %s" % (relative, line, description))
        if relative == NEW_FILE:
            continue
        scopes = [scope for _line, module, asname, scope in facts.new_imports if module == NEW_MODULE and asname in (None, NEW_NAME)]
        for line, kind, scope in facts.new_calls:
            if kind != "name":
                continue
            if not any(scope[: len(imported)] == imported for imported in scopes):
                fail("unbound", "%s:%d calls `%s` without importing it from `%s` in scope" % (relative, line, NEW_NAME, NEW_MODULE))
    if total_calls < base["calls"]:
        failures.append(
            "only %d call site(s) of `%s` found; the base commit has %d call site(s) of `%s` (%s)"
            % (total_calls, NEW_NAME, base["calls"], OLD_NAME, base["origin"])
        )
    if call_files < base["call_files"]:
        failures.append(
            "`%s` is called in %d file(s); the base commit calls `%s` in %d file(s)"
            % (NEW_NAME, call_files, OLD_NAME, base["call_files"])
        )

    # 5. The unrelated sibling function is untouched and nothing was mangled.
    sibling_refs = sum(facts.sibling_refs for facts in files.values())
    if sibling_refs < base["sibling_refs"]:
        failures.append("`%s` must be left alone: %d reference(s) found, base has %d" % (SIBLING, sibling_refs, base["sibling_refs"]))
    for relative in sorted(files):
        for line, identifier in files[relative].mangled:
            fail("mangled", "%s:%d suspicious identifier `%s` (blind textual replace?)" % (relative, line, identifier))

    # 6. Behaviour and the repository's own tests (only when the structure is right).
    tests_ran = 0
    if not failures:
        failures.extend(run_probe(root))
    if not failures and not skip_tests:
        test_failures, tests_ran = run_tests(root)
        failures.extend(test_failures)

    for kind, count in sorted(limited.items()):
        if count > 12:
            failures.append("... %d more `%s` finding(s) not shown" % (count - 12, kind))
    if failures:
        for index, failure in enumerate(failures, 1):
            print("%d. %s" % (index, failure))
        return 1
    print(
        "PASS: %d call sites of %s in %d files (base: %d in %d, from %s); %d Python files parse; %d repository tests ran"
        % (total_calls, NEW_NAME, call_files, base["calls"], base["call_files"], base["origin"], len(files), tests_ran)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
