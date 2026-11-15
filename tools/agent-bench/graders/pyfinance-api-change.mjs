#!/usr/bin/env node
// Held-out grader for the `pyfinance-api-change` task.
//
//   node pyfinance-api-change.mjs <worktree> [<source repo>]
//
// The task changes `validate_weights(weights, symbols, max_leverage,
// trading_dates=None, missing_weight_policy="zero")` into
// `validate_weights(weights, symbols, execution, *, trading_dates=None)` and
// updates every caller. Exit 0 = pass; failures are printed to stderr.
//
// 1. Structural check (`fixtures/pyfinance-api-change.check.py`, Python `ast`
//    over every Python file of the worktree): no call of `validate_weights`
//    uses the old interface, and the tests that called it before the change
//    still exist, are not skipped and still validate.
// 2. Held-out tests (`fixtures/pyfinance-api-change.heldout_test.py`): copied
//    to `<worktree>/tests/`, run from the worktree root, removed again. They
//    exercise the new interface and the unchanged behaviour, through
//    `validate_weights` and through `run_weight_backtest`.
// 3. The repository's own tests for the touched modules, selected by node id
//    so that only tests that exist and pass on the base commit in this
//    environment are run.
//
// Steps 2 and 3 use pytest. The interpreter is `AGENT_BENCH_PYTHON` or
// `python`. pytest is taken, in this order, from the interpreter itself, from
// the path list in `AGENT_BENCH_PYTEST_PATH`, or from the package cache of a
// conda installation found on PATH (the benchmark machine has pytest only
// there). When no pytest is found, step 2 runs the held-out file as a plain
// unittest script and step 3 is skipped with a note;
// `AGENT_BENCH_PYTEST_PATH=none` forces that mode.
//
// The project targets Python 3.11; a `StrEnum` backport is loaded for older
// interpreters so that `pyfinance.plugins.models` imports.
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HELD_OUT_REL = "tests/test_agent_bench_heldout_validate_weights.py";
const COMPAT_PLUGIN = "agent_bench_py39_compat";
// Tests of the base commit that pass there on Python 3.9 / pandas 2.0.1.
const REPO_TESTS = [
  "tests/test_backtest.py::test_close_to_close_weights_apply_to_next_return",
  "tests/test_backtest.py::test_turnover_costs_reduce_returns",
  "tests/test_backtest.py::test_backtest_rejects_explicit_missing_adjusted_close",
  "tests/test_backtest.py::test_backtest_rejects_incomplete_multi_symbol_price_panel",
  "tests/test_backtest.py::test_backtest_rejects_non_positive_adjusted_close",
  "tests/test_plugins.py::test_run_config_defaults_include_zero_cost_execution",
  "tests/test_plugins.py::test_run_config_defaults_symbol_resource_metadata_to_empty",
  "tests/test_plugins.py::test_plugin_selection_defaults_params_to_empty_dict",
  "tests/test_plugins.py::test_config_dataclasses_reject_attribute_assignment",
  "tests/test_plugins.py::test_plugin_kind_strategy_is_string_enum_value",
  "tests/test_plugins.py::test_plugin_definition_calls_generate_weights_with_context",
  "tests/test_plugins.py::test_plugin_context_adjusted_close_pivots_and_sorts_data",
  "tests/test_plugins.py::test_plugin_context_exposes_symbol_metadata_helpers",
  "tests/test_plugins.py::test_plugin_context_adjusted_close_raises_for_missing_selected_symbol",
  "tests/test_plugins.py::test_plugin_context_returns_pct_change_with_initial_row_filled",
  "tests/test_plugins.py::test_plugin_context_returns_preserves_non_initial_missing_prices",
  "tests/test_plugins.py::test_discover_plugins_loads_register_definitions_from_temp_plugin",
  "tests/test_plugins.py::test_discover_plugins_rejects_duplicate_plugin_ids",
  "tests/test_plugins.py::test_validate_weights_rejects_unknown_symbols",
  "tests/test_plugins.py::test_validate_weights_rejects_submitted_nan_weight",
  "tests/test_plugins.py::test_validate_weights_rejects_submitted_inf_weight",
  "tests/test_plugins.py::test_validate_weights_rejects_dates_outside_trading_calendar",
  "tests/test_plugins.py::test_validate_weights_fills_missing_trading_dates_with_zero_policy",
  "tests/test_plugins.py::test_validate_weights_allows_tiny_floating_point_leverage_overshoot",
  "tests/test_plugins.py::test_validate_weights_rejects_material_leverage_overshoot",
  "tests/test_strategy_plugins.py::test_inverse_volatility_caps_tiny_floating_point_leverage_overshoot",
];
const COMPAT_SOURCE = `import enum

if not hasattr(enum, "StrEnum"):

    class StrEnum(str, enum.Enum):
        def __str__(self):
            return str(self.value)

    enum.StrEnum = StrEnum
`;

const worktree = resolve(process.argv[2] ?? ".");
const here = dirname(fileURLToPath(import.meta.url));
const heldOutFixture = join(here, "fixtures", "pyfinance-api-change.heldout_test.py");
const checkScript = join(here, "fixtures", "pyfinance-api-change.check.py");
const heldOutPath = join(worktree, ...HELD_OUT_REL.split("/"));
const python = process.env.AGENT_BENCH_PYTHON || "python";
const deadline = Date.now() + 165_000; // the whole grader stays under 3 minutes

const failures = [];
const notes = [];

if (!existsSync(join(worktree, "src", "pyfinance"))) {
  failures.push(`not a PyFinance worktree: ${worktree}`);
} else if (!existsSync(heldOutFixture) || !existsSync(checkScript)) {
  failures.push(`grader fixtures not found next to ${here}`);
} else {
  const probe = run(["-c", "import sys, pandas, numpy; assert sys.version_info >= (3, 9)"], []);
  if (probe.error || probe.status !== 0) {
    failures.push(
      `cannot use Python interpreter "${python}" (needs Python >= 3.9 with pandas and numpy): ` +
        tail(probe.error?.message ?? output(probe), 5),
    );
  } else {
    grade();
  }
}

if (failures.length > 0) {
  console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
  for (const note of notes) console.error(`NOTE: ${note}`);
  process.exit(1);
}
for (const note of notes) console.log(`NOTE: ${note}`);
console.log(
  "PASS: validate_weights takes the ExecutionConfig, trading_dates is keyword-only, " +
    "every caller uses the new interface, behaviour unchanged",
);

function grade() {
  const structural = run([checkScript, worktree], []);
  if (structural.error || structural.status !== 0) {
    failures.push(`structural check failed:\n${tail(structural.error?.message ?? output(structural))}`);
  } else {
    notes.push(structural.stdout.trim());
  }

  const compatDir = mkdtempSync(join(tmpdir(), "agent-bench-pyfinance-"));
  const original = existsSync(heldOutPath) ? readFileSync(heldOutPath) : null;
  try {
    writeFileSync(join(compatDir, `${COMPAT_PLUGIN}.py`), COMPAT_SOURCE);
    writeFileSync(heldOutPath, readFileSync(heldOutFixture));
    const pytestPath = findPytest(compatDir);
    if (pytestPath === null) {
      notes.push(
        "pytest not found: held-out tests ran as a unittest script, the repository's own tests were not run",
      );
      report("held-out tests (unittest)", run([HELD_OUT_REL], []));
      return;
    }
    if (pytestPath.length > 0) notes.push(`pytest loaded from ${pytestPath[0]}`);
    const extraPath = [compatDir, ...pytestPath];
    const pytest = ["-m", "pytest", "-q", "--tb=short", "-o", "addopts=", "-p", COMPAT_PLUGIN];
    pytest.push("-p", "no:cacheprovider");
    report("held-out tests", run([...pytest, HELD_OUT_REL], extraPath));
    rmSync(heldOutPath, { force: true });
    report("repository tests of the touched modules", run([...pytest, ...REPO_TESTS], extraPath));
  } finally {
    if (original === null) rmSync(heldOutPath, { force: true });
    else writeFileSync(heldOutPath, original);
    rmSync(compatDir, { force: true, recursive: true });
  }
}

function report(label, result) {
  if (result.error) {
    failures.push(`${label} did not complete: ${result.error.message}`);
  } else if (result.status !== 0) {
    failures.push(`${label} failed (exit ${result.status}):\n${tail(output(result))}`);
  } else {
    const summary = output(result).match(/\d+ passed[^\r\n]*|Ran \d+ tests?[^\r\n]*/);
    notes.push(`${label}: ${summary ? summary[0].replace(/=+/g, "").trim() : "ok"}`);
  }
}

// Runs the interpreter in the worktree with `src` (and `extraPath`) importable.
// Entries of `extraPath` come after an inherited PYTHONPATH.
function run(args, extraPath, cwd = worktree) {
  const pythonPath = [join(worktree, "src"), process.env.PYTHONPATH, ...extraPath];
  return spawnSync(python, args, {
    cwd,
    env: {
      ...process.env,
      PYTHONPATH: pythonPath.filter(Boolean).join(delimiter),
      PYTHONDONTWRITEBYTECODE: "1",
      PYTHONIOENCODING: "utf-8",
    },
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    timeout: Math.max(1_000, Math.min(75_000, deadline - Date.now())),
    windowsHide: true,
  });
}

// Returns the extra PYTHONPATH entries needed to run pytest, or null. A
// candidate counts when it runs a one-test project with a `pyproject.toml`
// (so that the configuration parser it needs is importable too).
function findPytest(scratchDir) {
  const configured = process.env.AGENT_BENCH_PYTEST_PATH;
  if (configured && configured.trim().toLowerCase() === "none") return null;
  const probeDir = join(scratchDir, "probe");
  mkdirSync(probeDir);
  writeFileSync(join(probeDir, "pyproject.toml"), '[tool.pytest.ini_options]\naddopts = "-q"\n');
  writeFileSync(join(probeDir, "test_probe.py"), "def test_probe():\n    assert True\n");
  const candidates = [[]];
  if (configured) candidates.push(configured.split(delimiter).filter(Boolean));
  for (const candidate of candidates) {
    if (runsPytest(candidate, probeDir)) return candidate;
  }
  for (const candidate of condaCachePaths()) {
    if (runsPytest(candidate, probeDir)) return candidate;
  }
  return null;
}

function runsPytest(extraPath, probeDir) {
  const probe = run(["-m", "pytest", "-p", "no:cacheprovider", "test_probe.py"], extraPath, probeDir);
  return !probe.error && probe.status === 0;
}

// pytest and the dependencies the interpreter lacks, taken from the package
// cache (`<conda root>/pkgs/<name>-<version>-<build>/`) of conda installations
// on PATH. One candidate path list per cached pytest version, newest first.
function condaCachePaths() {
  const modules = {
    pluggy: "pluggy",
    iniconfig: "iniconfig",
    py: "py",
    toml: "toml",
    tomli: "tomli",
    atomicwrites: "atomicwrites",
    attr: "attrs",
    packaging: "packaging",
    colorama: "colorama",
    more_itertools: "more-itertools",
    exceptiongroup: "exceptiongroup",
  };
  const probe = run(
    [
      "-c",
      "import importlib.util as u, sys\n" +
        "print(' '.join(m for m in sys.argv[1:] if u.find_spec(m) is None))",
      ...Object.keys(modules),
    ],
    [],
  );
  if (probe.error || probe.status !== 0) return [];
  const missing = probe.stdout.trim().split(/\s+/).filter(Boolean);

  const roots = new Set();
  const pathEntries = (process.env.PATH ?? process.env.Path ?? "").split(delimiter);
  for (const entry of [process.env.CONDA_PREFIX, ...pathEntries]) {
    if (!entry) continue;
    for (const root of [entry, dirname(entry), dirname(dirname(entry))]) {
      if (existsSync(join(root, "conda-meta")) && existsSync(join(root, "pkgs"))) roots.add(root);
    }
  }

  const result = [];
  for (const root of roots) {
    const cache = join(root, "pkgs");
    const cached = (name) =>
      readdirSync(cache)
        .filter((entry) => entry.startsWith(`${name}-`) && /^\d/.test(entry.slice(name.length + 1)))
        .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
        .map((entry) => sitePackages(join(cache, entry)))
        .filter(Boolean);
    const dependencies = missing.map((module) => cached(modules[module])[0]).filter(Boolean);
    for (const pytestDir of cached("pytest")) result.push([pytestDir, ...dependencies]);
  }
  return result;
}

function sitePackages(packageDir) {
  const candidates = [join(packageDir, "Lib", "site-packages"), join(packageDir, "site-packages")];
  const lib = join(packageDir, "lib");
  if (existsSync(lib)) {
    for (const entry of readdirSync(lib)) {
      if (entry.startsWith("python")) candidates.push(join(lib, entry, "site-packages"));
    }
  }
  return candidates.find((candidate) => existsSync(candidate) && statSync(candidate).isDirectory());
}

function output(result) {
  return `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
}

function tail(text, lines = 60) {
  return String(text).trim().split(/\r?\n/).slice(-lines).join("\n");
}
