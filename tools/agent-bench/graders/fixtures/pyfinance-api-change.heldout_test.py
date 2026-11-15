"""Held-out tests for the `pyfinance-api-change` benchmark task.

The grader copies this file into `<worktree>/tests/`, runs it from the
worktree root and removes it again. It is never part of the tree the agent
sees. It runs under pytest and as a plain script (`python <file>`), so it
only uses `unittest`.

It checks the NEW interface of `validate_weights`

    validate_weights(weights, symbols, execution, *, trading_dates=None)

and that everything else behaves as it did before the change.
"""
from __future__ import annotations

import enum
import inspect
import re
import sys
import unittest
from dataclasses import FrozenInstanceError, fields
from datetime import date

if not hasattr(enum, "StrEnum"):  # Python < 3.11: the project needs StrEnum

    class _StrEnum(str, enum.Enum):
        def __str__(self) -> str:
            return str(self.value)

    _StrEnum.__name__ = _StrEnum.__qualname__ = "StrEnum"
    enum.StrEnum = _StrEnum

import pandas as pd

import pyfinance.backtest.engine  # noqa: F401  (loaded so the spy can patch it)
import pyfinance.plugins.validation as validation_module
from pyfinance.backtest.engine import run_weight_backtest
from pyfinance.config import ExecutionConfig
from pyfinance.plugins.validation import validate_weights

DAY1 = pd.Timestamp("2024-01-02")
DAY2 = pd.Timestamp("2024-01-03")
DAY3 = pd.Timestamp("2024-01-04")


def _frame(columns: dict, dates=(DAY1,)) -> pd.DataFrame:
    return pd.DataFrame(columns, index=pd.DatetimeIndex(list(dates)))


def _prices() -> pd.DataFrame:
    rows = []
    for symbol, closes in {"SPY": [100.0, 102.0, 103.0], "QQQ": [200.0, 202.0, 206.04]}.items():
        for day, close in zip([DAY1, DAY2, DAY3], closes):
            rows.append({"date": day, "symbol": symbol, "adj_close": close})
    return pd.DataFrame(rows)


def _backtest(weights: pd.DataFrame, execution: ExecutionConfig, symbols=("SPY",)):
    prices = _prices()
    return run_weight_backtest(
        name="held-out",
        prices=prices[prices["symbol"].isin(list(symbols))],
        weights=weights,
        execution=execution,
        start=date(2024, 1, 2),
        end=date(2024, 1, 4),
    )


class NewSignatureTests(unittest.TestCase):
    def test_signature_is_exactly_the_requested_one(self) -> None:
        parameters = list(inspect.signature(validate_weights).parameters.values())
        described = [
            (p.name, p.kind.name, "required" if p.default is p.empty else repr(p.default))
            for p in parameters
        ]
        self.assertEqual(
            described,
            [
                ("weights", "POSITIONAL_OR_KEYWORD", "required"),
                ("symbols", "POSITIONAL_OR_KEYWORD", "required"),
                ("execution", "POSITIONAL_OR_KEYWORD", "required"),
                ("trading_dates", "KEYWORD_ONLY", "None"),
            ],
        )

    def test_execution_can_be_passed_by_position_or_by_keyword(self) -> None:
        weights = _frame({"AAPL": [0.4], "MSFT": [0.6]})
        by_position = validate_weights(weights, ["AAPL", "MSFT"], ExecutionConfig())
        by_keyword = validate_weights(
            weights=weights, symbols=["AAPL", "MSFT"], execution=ExecutionConfig()
        )
        pd.testing.assert_frame_equal(by_position, weights)
        pd.testing.assert_frame_equal(by_keyword, weights)

    def test_execution_is_required(self) -> None:
        with self.assertRaises(TypeError):
            validate_weights(_frame({"AAPL": [1.0]}), ["AAPL"])

    def test_trading_dates_is_keyword_only(self) -> None:
        weights = _frame({"AAPL": [1.0]})
        dates = pd.DatetimeIndex([DAY1, DAY2])
        with self.assertRaises(TypeError):
            validate_weights(weights, ["AAPL"], ExecutionConfig(), dates)
        validated = validate_weights(
            weights, ["AAPL"], ExecutionConfig(), trading_dates=dates
        )
        pd.testing.assert_frame_equal(validated, _frame({"AAPL": [1.0, 0.0]}, dates))

    def test_old_keywords_are_rejected(self) -> None:
        weights = _frame({"AAPL": [1.0]})
        with self.assertRaises(TypeError):
            validate_weights(weights, symbols=["AAPL"], max_leverage=1.0)
        with self.assertRaises(TypeError):
            validate_weights(weights, ["AAPL"], ExecutionConfig(), max_leverage=1.0)
        with self.assertRaises(TypeError):
            validate_weights(
                weights, ["AAPL"], ExecutionConfig(), missing_weight_policy="zero"
            )
        with self.assertRaises(TypeError):
            validate_weights(
                weights,
                symbols=["AAPL"],
                max_leverage=1.0,
                trading_dates=pd.DatetimeIndex([DAY1]),
                missing_weight_policy="zero",
            )


class SettingsComeFromExecutionTests(unittest.TestCase):
    def test_default_config_limits_leverage_to_one(self) -> None:
        weights = _frame({"AAPL": [0.5, 0.75], "MSFT": [0.5, 0.75]}, (DAY1, DAY2))
        with self.assertRaises(ValueError) as caught:
            validate_weights(weights, ["AAPL", "MSFT"], ExecutionConfig())
        self.assertEqual(
            str(caught.exception),
            "weights exceed max_leverage 1.0 on 2024-01-03 00:00:00: 1.5",
        )

    def test_leverage_limit_is_read_from_the_config(self) -> None:
        weights = _frame({"AAPL": [0.75], "MSFT": [0.75]})
        validated = validate_weights(
            weights, ["AAPL", "MSFT"], ExecutionConfig(max_leverage=2.0)
        )
        pd.testing.assert_frame_equal(validated, weights)

        with self.assertRaises(ValueError) as caught:
            validate_weights(
                _frame({"AAPL": [0.75]}), ["AAPL"], ExecutionConfig(max_leverage=0.5)
            )
        self.assertEqual(
            str(caught.exception),
            "weights exceed max_leverage 0.5 on 2024-01-02 00:00:00: 0.75",
        )

    def test_leverage_counts_short_positions_by_absolute_value(self) -> None:
        weights = _frame({"AAPL": [0.75], "MSFT": [-0.75]})
        with self.assertRaisesRegex(ValueError, r"weights exceed max_leverage 1\.0 on"):
            validate_weights(weights, ["AAPL", "MSFT"], ExecutionConfig())
        validated = validate_weights(
            weights, ["AAPL", "MSFT"], ExecutionConfig(max_leverage=1.5)
        )
        pd.testing.assert_frame_equal(validated, weights)

    def test_leverage_at_or_a_rounding_error_above_the_limit_is_accepted(self) -> None:
        at_limit = _frame({"AAPL": [0.5], "MSFT": [0.5]})
        pd.testing.assert_frame_equal(
            validate_weights(at_limit, ["AAPL", "MSFT"], ExecutionConfig()), at_limit
        )
        overshoot = _frame({"AAPL": [float.fromhex("0x1.0000000000001p+0")]})
        pd.testing.assert_frame_equal(
            validate_weights(overshoot, ["AAPL"], ExecutionConfig()), overshoot
        )

    def test_missing_weight_policy_is_read_from_the_config(self) -> None:
        weights = _frame({"AAPL": [1.0]})
        with self.assertRaises(ValueError) as caught:
            validate_weights(
                weights, ["AAPL"], ExecutionConfig(missing_weight_policy="ffill")
            )
        self.assertEqual(str(caught.exception), "Unsupported missing_weight_policy: ffill")
        validated = validate_weights(
            weights, ["AAPL"], ExecutionConfig(missing_weight_policy="zero")
        )
        pd.testing.assert_frame_equal(validated, weights)

    def test_cost_settings_do_not_affect_validation(self) -> None:
        weights = _frame({"AAPL": [0.25], "MSFT": [0.5]})
        validated = validate_weights(
            weights, ["AAPL", "MSFT"], ExecutionConfig(fee_bps=25.0, slippage_bps=5.0)
        )
        pd.testing.assert_frame_equal(validated, weights)


class UnchangedValidationTests(unittest.TestCase):
    def _assert_rejected(self, message: str, weights, symbols, **kwargs) -> None:
        with self.assertRaises(ValueError) as caught:
            validate_weights(weights, symbols, ExecutionConfig(), **kwargs)
        self.assertEqual(str(caught.exception), message)

    def test_weights_need_a_datetime_index(self) -> None:
        self._assert_rejected(
            "weights must use a DatetimeIndex",
            pd.DataFrame({"AAPL": [1.0]}, index=["2024-01-02"]),
            ["AAPL"],
        )

    def test_unknown_symbols_are_listed(self) -> None:
        self._assert_rejected(
            "weights contain unknown symbols: TSLA, NFLX",
            _frame({"AAPL": [0.2], "TSLA": [0.2], "NFLX": [0.2]}),
            ["AAPL", "MSFT"],
        )

    def test_trading_dates_must_be_a_datetime_index(self) -> None:
        self._assert_rejected(
            "trading_dates must be a DatetimeIndex",
            _frame({"AAPL": [1.0]}),
            ["AAPL"],
            trading_dates=[DAY1],
        )

    def test_nan_and_infinite_weights_are_rejected(self) -> None:
        for bad in (float("nan"), float("inf"), float("-inf")):
            self._assert_rejected(
                "weights must contain finite numeric values",
                _frame({"AAPL": [bad]}),
                ["AAPL"],
            )

    def test_non_numeric_weights_are_rejected(self) -> None:
        with self.assertRaises(ValueError):
            validate_weights(_frame({"AAPL": ["heavy"]}), ["AAPL"], ExecutionConfig())

    def test_numeric_strings_are_converted(self) -> None:
        validated = validate_weights(
            _frame({"AAPL": ["0.25"], "MSFT": ["0.5"]}), ["AAPL", "MSFT"], ExecutionConfig()
        )
        pd.testing.assert_frame_equal(validated, _frame({"AAPL": [0.25], "MSFT": [0.5]}))

    def test_dates_outside_the_trading_calendar_are_listed(self) -> None:
        self._assert_rejected(
            "weights contain dates outside trading calendar: 2024-01-04 00:00:00",
            _frame({"AAPL": [0.5, 0.5]}, (DAY1, DAY3)),
            ["AAPL"],
            trading_dates=pd.DatetimeIndex([DAY1, DAY2]),
        )

    def test_missing_symbols_and_dates_are_filled_with_zero(self) -> None:
        trading_dates = pd.DatetimeIndex([DAY1, DAY2, DAY3], name="date")
        validated = validate_weights(
            _frame({"MSFT": [0.25], "AAPL": [0.5]}, (DAY2,)),
            ("AAPL", "MSFT", "QQQ"),
            ExecutionConfig(),
            trading_dates=trading_dates,
        )
        expected = pd.DataFrame(
            {"AAPL": [0.0, 0.5, 0.0], "MSFT": [0.0, 0.25, 0.0], "QQQ": [0.0, 0.0, 0.0]},
            index=trading_dates,
        )
        pd.testing.assert_frame_equal(validated, expected)

    def test_without_trading_dates_the_submitted_dates_are_kept(self) -> None:
        validated = validate_weights(
            _frame({"MSFT": [0.25, 0.5]}, (DAY1, DAY3)), ["AAPL", "MSFT"], ExecutionConfig()
        )
        pd.testing.assert_frame_equal(
            validated, _frame({"AAPL": [0.0, 0.0], "MSFT": [0.25, 0.5]}, (DAY1, DAY3))
        )


class BacktestStillValidatesTests(unittest.TestCase):
    def test_backtest_numbers_are_unchanged(self) -> None:
        weights = _frame({"SPY": [0.0, 1.0, 1.0]}, (DAY1, DAY2, DAY3))
        result = _backtest(weights, ExecutionConfig(fee_bps=10.0, slippage_bps=5.0))
        self.assertEqual(result.name, "held-out")
        pd.testing.assert_frame_equal(result.weights, weights, check_names=False)
        self.assertAlmostEqual(result.turnover.loc[DAY3], 1.0)
        self.assertAlmostEqual(result.costs.loc[DAY3], 0.0015)
        self.assertAlmostEqual(result.returns.loc[DAY2], 0.0)
        self.assertAlmostEqual(result.returns.loc[DAY3], 1.0 / 102.0 - 0.0015)
        self.assertAlmostEqual(result.equity_curve.iloc[-1], 1.0 + 1.0 / 102.0 - 0.0015)

    def test_backtest_fills_missing_weight_dates_with_zero(self) -> None:
        result = _backtest(_frame({"SPY": [1.0]}, (DAY1,)), ExecutionConfig())
        self.assertEqual(result.weights["SPY"].tolist(), [1.0, 0.0, 0.0])
        self.assertEqual(list(result.weights.index), [DAY1, DAY2, DAY3])
        self.assertAlmostEqual(result.returns.loc[DAY2], 0.02)
        self.assertAlmostEqual(result.returns.loc[DAY3], 0.0)

    def test_backtest_enforces_the_configured_leverage_limit(self) -> None:
        weights = _frame({"SPY": [1.5, 1.5, 1.5]}, (DAY1, DAY2, DAY3))
        with self.assertRaisesRegex(
            ValueError, re.escape("weights exceed max_leverage 1.0 on 2024-01-02 00:00:00: 1.5")
        ):
            _backtest(weights, ExecutionConfig())
        result = _backtest(weights, ExecutionConfig(max_leverage=2.0))
        self.assertAlmostEqual(result.returns.loc[DAY2], 1.5 * 0.02)
        self.assertAlmostEqual(result.returns.loc[DAY3], 1.5 / 102.0)

    def test_backtest_enforces_the_configured_missing_weight_policy(self) -> None:
        weights = _frame({"SPY": [1.0, 1.0, 1.0]}, (DAY1, DAY2, DAY3))
        with self.assertRaisesRegex(ValueError, "Unsupported missing_weight_policy: drop"):
            _backtest(weights, ExecutionConfig(missing_weight_policy="drop"))

    def test_backtest_rejects_weights_outside_the_price_calendar(self) -> None:
        weights = _frame({"SPY": [1.0, 1.0]}, (DAY1, pd.Timestamp("2024-01-06")))
        with self.assertRaisesRegex(ValueError, "outside trading calendar: 2024-01-06"):
            _backtest(weights, ExecutionConfig())

    def test_backtest_rejects_weights_for_symbols_without_prices(self) -> None:
        weights = _frame({"SPY": [0.5], "IWM": [0.5]})
        with self.assertRaisesRegex(ValueError, "weights contain unknown symbols: IWM"):
            _backtest(weights, ExecutionConfig(), symbols=("SPY", "QQQ"))

    def test_backtest_hands_its_own_config_to_validate_weights(self) -> None:
        real = validation_module.validate_weights
        calls = []

        def spy(*args, **kwargs):
            calls.append(inspect.signature(real).bind(*args, **kwargs).arguments)
            return real(*args, **kwargs)

        patched = []
        for name, module in list(sys.modules.items()):
            if name.split(".")[0] != "pyfinance" or module is None:
                continue
            if getattr(module, "validate_weights", None) is real:
                module.validate_weights = spy
                patched.append(module)
        config = ExecutionConfig(fee_bps=10.0, slippage_bps=5.0, max_leverage=1.5)
        weights = _frame({"SPY": [0.5, 0.5, 0.5], "QQQ": [0.75, 0.75, 0.75]}, (DAY1, DAY2, DAY3))
        try:
            _backtest(weights, config, symbols=("SPY", "QQQ"))
        finally:
            for module in patched:
                module.validate_weights = real

        self.assertEqual(len(calls), 1, "run_weight_backtest must validate the weights once")
        arguments = calls[0]
        self.assertEqual(
            arguments["execution"],
            config,
            "run_weight_backtest must pass its ExecutionConfig through unchanged",
        )
        self.assertEqual(list(arguments["symbols"]), ["QQQ", "SPY"])
        self.assertEqual(list(arguments["trading_dates"]), [DAY1, DAY2, DAY3])


class ExecutionConfigUnchangedTests(unittest.TestCase):
    def test_fields_and_defaults_are_unchanged(self) -> None:
        self.assertEqual(
            [(field.name, field.default) for field in fields(ExecutionConfig)],
            [
                ("fee_bps", 0.0),
                ("slippage_bps", 0.0),
                ("max_leverage", 1.0),
                ("missing_weight_policy", "zero"),
            ],
        )

    def test_config_is_still_frozen(self) -> None:
        with self.assertRaises(FrozenInstanceError):
            ExecutionConfig().max_leverage = 2.0


if __name__ == "__main__":
    unittest.main(verbosity=1)
