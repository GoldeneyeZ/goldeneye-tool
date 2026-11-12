"""Held-out grader test for the `pyfinance-behavior-bug` benchmark task.

Copied into `tests/` of the graded worktree by
`graders/pyfinance-behavior-bug.mjs`, run, and removed again. Written as
`unittest.TestCase` classes so that it runs under `python -m pytest` and, when
pytest is not installed, as a plain script (`python <file>`).

Bug under test: price dates that carry a UTC offset were converted to UTC
before the offset was dropped, so a daily bar moved to another day or to a
non-midnight time. Expected: the wall-clock date of the row's own timezone.
"""
from __future__ import annotations

import tempfile
import unittest
from datetime import date
from pathlib import Path

import pandas as pd

from pyfinance.backtest.engine import run_weight_backtest
from pyfinance.config import ExecutionConfig
from pyfinance.data.normalization import normalize_price_frame, read_local_prices

FIXTURE_PATH = Path(__file__).resolve().parent / "fixtures" / "prices.csv"


def _frame(rows):
    return pd.DataFrame(
        {
            "date": [row[0] for row in rows],
            "symbol": [row[1] for row in rows],
            "open": [row[2] for row in rows],
            "high": [row[2] for row in rows],
            "low": [row[2] for row in rows],
            "close": [row[2] for row in rows],
            "adj_close": [row[2] for row in rows],
            "volume": [1000 for _row in rows],
        }
    )


def _timestamps(values):
    return [pd.Timestamp(value) for value in values]


class OffsetDatesKeepTheirCalendarDay(unittest.TestCase):
    def _assert_naive_datetimes(self, prices):
        self.assertTrue(pd.api.types.is_datetime64_any_dtype(prices["date"]))
        self.assertIsNone(prices["date"].dt.tz)

    def test_positive_offset_strings_keep_their_day(self):
        prices = normalize_price_frame(
            _frame(
                [
                    ("2024-01-02 00:00:00+01:00", "MC.PA", 100.0),
                    ("2024-01-03 00:00:00+01:00", "MC.PA", 101.0),
                ]
            )
        )

        self._assert_naive_datetimes(prices)
        self.assertEqual(
            prices["date"].tolist(), _timestamps(["2024-01-02", "2024-01-03"])
        )
        self.assertEqual(prices["adj_close"].tolist(), [100.0, 101.0])

    def test_negative_offsets_across_a_dst_change_stay_at_midnight(self):
        prices = normalize_price_frame(
            _frame(
                [
                    ("2024-03-08 00:00:00-05:00", "SPY", 100.0),
                    ("2024-03-11 00:00:00-04:00", "SPY", 101.0),
                    ("2024-03-12 00:00:00-04:00", "SPY", 102.0),
                ]
            )
        )

        self._assert_naive_datetimes(prices)
        self.assertEqual(
            prices["date"].tolist(),
            _timestamps(["2024-03-08", "2024-03-11", "2024-03-12"]),
        )

    def test_rows_from_several_timezones_share_the_same_trading_day(self):
        prices = normalize_price_frame(
            _frame(
                [
                    ("2024-01-02 00:00:00-05:00", "SPY", 470.0),
                    ("2024-01-03 00:00:00-05:00", "SPY", 471.0),
                    ("2024-01-02 00:00:00+01:00", "MC.PA", 700.0),
                    ("2024-01-03 00:00:00+01:00", "MC.PA", 701.0),
                    ("2024-01-02 00:00:00+09:00", "7203.T", 2600.0),
                    ("2024-01-03 00:00:00+09:00", "7203.T", 2601.0),
                ]
            )
        )

        self._assert_naive_datetimes(prices)
        table = prices.pivot(index="date", columns="symbol", values="adj_close")
        self.assertEqual(
            list(table.index), _timestamps(["2024-01-02", "2024-01-03"])
        )
        self.assertFalse(table.isna().any().any())
        self.assertEqual(table.loc[pd.Timestamp("2024-01-03"), "7203.T"], 2601.0)

    def test_timezone_aware_datetime_column_keeps_local_day(self):
        frame = _frame(
            [
                ("2024-01-02", "MC.PA", 100.0),
                ("2024-01-03", "MC.PA", 101.0),
            ]
        )
        frame["date"] = pd.to_datetime(frame["date"]).dt.tz_localize("Europe/Paris")

        prices = normalize_price_frame(frame)

        self._assert_naive_datetimes(prices)
        self.assertEqual(
            prices["date"].tolist(), _timestamps(["2024-01-02", "2024-01-03"])
        )

    def test_imported_csv_with_offsets_backtests_over_the_whole_range(self):
        csv_text = (
            "Date,Symbol,Open,High,Low,Close,Adj Close,Volume\n"
            "2024-01-02 00:00:00+01:00,mc.pa,100,100,100,100,100,1000\n"
            "2024-01-03 00:00:00+01:00,mc.pa,110,110,110,110,110,1000\n"
            "2024-01-04 00:00:00+01:00,mc.pa,121,121,121,121,121,1000\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "paris.csv"
            path.write_text(csv_text)
            prices = read_local_prices(path)

        self.assertEqual(
            prices["date"].tolist(),
            _timestamps(["2024-01-02", "2024-01-03", "2024-01-04"]),
        )
        weights = pd.DataFrame(
            {"MC.PA": [1.0, 1.0, 1.0]},
            index=pd.DatetimeIndex(["2024-01-02", "2024-01-03", "2024-01-04"]),
        )
        result = run_weight_backtest(
            name="hold",
            prices=prices,
            weights=weights,
            execution=ExecutionConfig(),
            start=date(2024, 1, 2),
            end=date(2024, 1, 4),
        )

        self.assertEqual(
            list(result.returns.index),
            _timestamps(["2024-01-02", "2024-01-03", "2024-01-04"]),
        )
        self.assertAlmostEqual(result.equity_curve.iloc[-1], 1.21, places=9)


class ExistingBehaviourIsPreserved(unittest.TestCase):
    def test_dates_without_offset_are_unchanged(self):
        prices = normalize_price_frame(
            _frame(
                [
                    ("2020-01-03", "spy", 103.0),
                    ("2020-01-02", "spy", 102.0),
                    ("2020-01-02", "qqq", 204.0),
                ]
            )
        )

        self.assertTrue(pd.api.types.is_datetime64_any_dtype(prices["date"]))
        self.assertIsNone(prices["date"].dt.tz)
        self.assertEqual(prices["symbol"].tolist(), ["QQQ", "SPY", "SPY"])
        self.assertEqual(
            prices["date"].tolist(),
            _timestamps(["2020-01-02", "2020-01-02", "2020-01-03"]),
        )
        self.assertEqual(prices["adj_close"].tolist(), [204.0, 102.0, 103.0])
        self.assertEqual(str(prices["adj_close"].dtype), "float64")

    def test_naive_datetime_column_is_unchanged(self):
        frame = _frame(
            [
                ("2020-01-02", "SPY", 102.0),
                ("2020-01-03", "SPY", 103.0),
            ]
        )
        frame["date"] = pd.to_datetime(frame["date"])

        prices = normalize_price_frame(frame)

        self.assertIsNone(prices["date"].dt.tz)
        self.assertEqual(
            prices["date"].tolist(), _timestamps(["2020-01-02", "2020-01-03"])
        )

    def test_utc_marked_dates_keep_their_day(self):
        prices = normalize_price_frame(
            _frame(
                [
                    ("2024-01-02T00:00:00Z", "SPY", 100.0),
                    ("2024-01-03T00:00:00+00:00", "SPY", 101.0),
                ]
            )
        )

        self.assertIsNone(prices["date"].dt.tz)
        self.assertEqual(
            prices["date"].tolist(), _timestamps(["2024-01-02", "2024-01-03"])
        )

    def test_repository_fixture_still_loads(self):
        prices = read_local_prices(FIXTURE_PATH)

        self.assertEqual(len(prices), 6)
        self.assertEqual(
            sorted(set(prices["date"].tolist())),
            _timestamps(["2020-01-01", "2020-01-02", "2020-01-03"]),
        )
        self.assertEqual(prices["symbol"].tolist(), ["QQQ"] * 3 + ["SPY"] * 3)

    def test_missing_required_column_is_still_rejected(self):
        frame = _frame([("2024-01-02 00:00:00+01:00", "MC.PA", 100.0)]).drop(
            columns=["adj_close"]
        )

        with self.assertRaises(ValueError) as raised:
            normalize_price_frame(frame)
        self.assertIn("adj_close", str(raised.exception))


if __name__ == "__main__":
    unittest.main(verbosity=2)
