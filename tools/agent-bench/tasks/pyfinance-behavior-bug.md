Bug report: price rows end up on the wrong day when their dates carry a UTC offset.

What I do: I load daily prices whose `date` values include a timezone offset.
This happens with a CSV exported from Yahoo history data and with a provider
that returns timezone-aware dates. For example a Paris-listed symbol has rows
like `2024-01-02 00:00:00+01:00`, and a New York symbol has
`2024-03-08 00:00:00-05:00` followed, after the DST change, by
`2024-03-11 00:00:00-04:00`.

What I get: after loading, the Paris row is dated `2024-01-01 23:00:00` (the
previous day) and the New York rows are dated `2024-03-08 05:00:00` and
`2024-03-11 04:00:00`. Downstream this breaks runs: the last requested day is
silently dropped from a backtest, weights dated `2024-01-02` are rejected as
being outside the trading calendar, and symbols from two exchanges no longer
line up on the same dates.

What I expect: a price row keeps the calendar date and time written in its own
timezone, with the offset simply dropped. `2024-01-02 00:00:00+01:00` must
become `2024-01-02 00:00:00`, and both New York rows above must stay at
midnight of their own day. This must hold wherever prices enter the
application (local files, provider downloads, cached data), for date strings
with an offset and for timezone-aware datetime values, and for data that mixes
several offsets in one load.

Keep everything else as it is: dates without an offset are unchanged, UTC
dates (`Z` or `+00:00`) keep their day, the `date` column stays a
timezone-naive datetime column, and sorting, symbol upper-casing, numeric
conversion and the missing-column error are unchanged.

The environment this runs in has Python 3.9 and pandas 2.0.1; the fix and the
test must work there.

Find the cause, fix it, and add a regression test next to the existing tests
for this area. Keep the change focused, do not add dependencies, and do not
change public function signatures. Do not run the test suite; a held-out test
is run after your response.
