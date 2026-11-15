"""Structural check for the `pyfinance-api-change` benchmark task.

    python pyfinance-api-change.check.py <worktree>

Walks every Python file of the worktree (tracked, or untracked and not
ignored) with `ast` and fails (exit 1, one line per problem) when

- a file under `src/`, `tests/` or `plugins/` no longer parses;
- a call of `validate_weights` still uses the old interface: a
  `max_leverage=` or `missing_weight_policy=` keyword, more than three
  positional arguments (`trading_dates` is keyword-only now), a bare number
  where the config goes, or no `execution` argument at all (calls written
  inside `pytest.raises(TypeError)` / `assertRaises(TypeError)` to show that
  the old interface is rejected are not callers and are ignored);
- one of the tests that exercised `validate_weights` before the change is
  gone, skipped, or no longer validates anything.

It needs nothing but the standard library.
"""
from __future__ import annotations

import ast
import os
import subprocess
import sys
from pathlib import Path

FUNCTION = "validate_weights"
OLD_KEYWORDS = ("max_leverage", "missing_weight_policy")
HELD_OUT = "tests/test_agent_bench_heldout_validate_weights.py"
MUST_PARSE = ("src/", "tests/", "plugins/")
SKIP_DIRS = {".git", ".idea", ".venv", "venv", "node_modules", "__pycache__", ".pytest_cache"}

# Tests that called validate_weights on the base commit: file -> function names.
BASE_TESTS = {
    "tests/test_plugins.py": (
        "test_validate_weights_rejects_unknown_symbols",
        "test_validate_weights_rejects_submitted_nan_weight",
        "test_validate_weights_rejects_submitted_inf_weight",
        "test_validate_weights_rejects_dates_outside_trading_calendar",
        "test_validate_weights_fills_missing_trading_dates_with_zero_policy",
        "test_validate_weights_allows_tiny_floating_point_leverage_overshoot",
        "test_validate_weights_rejects_material_leverage_overshoot",
    ),
    "tests/test_strategy_plugins.py": (
        "test_inverse_volatility_caps_tiny_floating_point_leverage_overshoot",
    ),
}


def python_files(root: Path) -> list[str]:
    try:
        listed = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-co", "--exclude-standard", "-z", "--", "*.py"],
            capture_output=True,
            check=True,
            timeout=60,
        ).stdout.decode("utf-8", "replace")
        paths = [path for path in listed.split("\0") if path]
    except (OSError, subprocess.SubprocessError):
        paths = []
        for current, dirs, files in os.walk(root):
            dirs[:] = [name for name in dirs if name not in SKIP_DIRS]
            for name in files:
                if name.endswith(".py"):
                    paths.append(Path(current, name).relative_to(root).as_posix())
    return sorted(
        path
        for path in {path.replace("\\", "/") for path in paths}
        if path != HELD_OUT and (root / path).is_file()
    )


def callee_name(node: ast.AST) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    return None


def is_number(node: ast.AST) -> bool:
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub)):
        node = node.operand
    return (
        isinstance(node, ast.Constant)
        and isinstance(node.value, (int, float))
        and not isinstance(node.value, bool)
    )


def expects_type_error(call: ast.Call) -> bool:
    """`pytest.raises(TypeError, ...)` / `self.assertRaises(TypeError, ...)`."""
    if callee_name(call.func) not in ("raises", "assertRaises", "assertRaisesRegex"):
        return False
    expected = call.args[:1] + [k.value for k in call.keywords if k.arg == "expected_exception"]
    names = []
    for node in expected:
        names.extend(node.elts if isinstance(node, ast.Tuple) else [node])
    return bool(names) and all(callee_name(node) == "TypeError" for node in names)


class CallChecker(ast.NodeVisitor):
    """Checks every call of validate_weights in one file.

    A call written to show that the old interface is rejected (inside a block
    or call that expects `TypeError`) is not a caller and is left alone.
    """

    def __init__(self, path: str, failures: list[str]) -> None:
        self.path = path
        self.failures = failures
        self.call_sites = 0
        self.negative = 0

    def visit_With(self, node: ast.With) -> None:
        negative = any(
            isinstance(item.context_expr, ast.Call) and expects_type_error(item.context_expr)
            for item in node.items
        )
        for item in node.items:
            self.visit(item)
        self.negative += negative
        for statement in node.body:
            self.visit(statement)
        self.negative -= negative

    visit_AsyncWith = visit_With

    def visit_Call(self, node: ast.Call) -> None:
        negative = expects_type_error(node)
        if not negative and not self.negative and check_call(self.path, node, self.failures):
            self.call_sites += 1
        self.negative += negative
        self.generic_visit(node)
        self.negative -= negative


def check_call(path: str, call: ast.Call, failures: list[str]) -> bool:
    """Checks one call. Returns True when it is a call of validate_weights."""
    where = f"{path}:{call.lineno}"
    keywords = [keyword.arg for keyword in call.keywords]
    if callee_name(call.func) != FUNCTION:
        # functools.partial(validate_weights, max_leverage=...) and the like
        if any(callee_name(arg) == FUNCTION for arg in call.args):
            old = [name for name in keywords if name in OLD_KEYWORDS]
            if old:
                failures.append(f"{where}: {FUNCTION} is still bound with {', '.join(old)}")
        return False

    old = [name for name in keywords if name in OLD_KEYWORDS]
    if old:
        failures.append(
            f"{where}: call of {FUNCTION} still passes the old keyword(s) {', '.join(old)}"
        )
    starred = any(isinstance(arg, ast.Starred) for arg in call.args) or None in keywords
    if starred:
        return True
    if len(call.args) > 3:
        failures.append(
            f"{where}: call of {FUNCTION} passes {len(call.args)} positional arguments "
            "(trading_dates is keyword-only)"
        )
    if len(call.args) >= 3 and is_number(call.args[2]):
        failures.append(
            f"{where}: call of {FUNCTION} passes a number where the ExecutionConfig goes"
        )
    if len(call.args) < 3 and "execution" not in keywords and not old:
        failures.append(f"{where}: call of {FUNCTION} does not pass `execution`")
    return True


def check_base_tests(path: str, tree: ast.Module, failures: list[str]) -> None:
    functions = {
        node.name: node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }
    for name in BASE_TESTS[path]:
        function = functions.get(name)
        if function is None:
            failures.append(f"{path}: existing test `{name}` was removed or renamed")
            continue
        marks = " ".join(ast.dump(decorator) for decorator in function.decorator_list)
        calls = [node for node in ast.walk(function) if isinstance(node, ast.Call)]
        called = [callee_name(call.func) or "" for call in calls]
        if "skip" in marks or "xfail" in marks or any(n in ("skip", "xfail") for n in called):
            failures.append(f"{path}: existing test `{name}` is skipped or expected to fail")
        if not any("validate" in callee for callee in called):
            failures.append(f"{path}: existing test `{name}` no longer validates any weights")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pyfinance-api-change.check.py <worktree>")
        return 2
    root = Path(sys.argv[1]).resolve()
    failures: list[str] = []
    call_sites: dict[str, int] = {}
    seen_tests = set()

    for path in python_files(root):
        try:
            source = (root / path).read_text(encoding="utf-8-sig")
            tree = ast.parse(source, filename=path)
        except (SyntaxError, UnicodeDecodeError, ValueError) as error:
            if path.startswith(MUST_PARSE):
                failures.append(f"{path}: does not parse: {error}")
            continue
        checker = CallChecker(path, failures)
        checker.visit(tree)
        if checker.call_sites:
            call_sites[path] = checker.call_sites
        if path in BASE_TESTS:
            seen_tests.add(path)
            check_base_tests(path, tree, failures)

    for path in BASE_TESTS:
        if path not in seen_tests and not any(f.startswith(f"{path}:") for f in failures):
            failures.append(f"{path}: test file is missing")
    if not any(path.startswith("src/") for path in call_sites):
        failures.append(f"no call of {FUNCTION} is left under src/ (the backtest must validate)")

    if failures:
        print("\n".join(failures))
        return 1
    total = sum(call_sites.values())
    print(
        f"structural check OK: {total} call sites of {FUNCTION} in {len(call_sites)} files "
        "use the new interface"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
