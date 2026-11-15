Change the interface of `validate_weights`, the function that checks the
weights a plugin returns before they are backtested.

Today it takes the leverage limit and the missing-weight policy as two loose
arguments, `max_leverage` and `missing_weight_policy`. Both are fields of
`ExecutionConfig`. A caller that holds a config has to unpack it field by
field, and a caller that leaves the policy out silently validates against the
default instead of the run's setting. More execution limits are coming, and we
do not want one more loose parameter threaded through every caller per limit.

Target state:

1. The signature is exactly this, and the function stays where it is:

   ```python
   def validate_weights(
       weights: pd.DataFrame,
       symbols: Sequence[str],
       execution: ExecutionConfig,
       *,
       trading_dates: pd.DatetimeIndex | None = None,
   ) -> pd.DataFrame:
   ```

   The leverage limit and the missing-weight policy are read from `execution`.
2. The old parameters are gone: `max_leverage=` and `missing_weight_policy=`
   are rejected like any unknown keyword. No alias, wrapper, `**kwargs`,
   default value for `execution`, or other compatibility path.
3. Update every caller in the repository's Python code, tests included. A
   caller that already holds an `ExecutionConfig` passes that object through;
   it must not build a new one from some of its fields.
4. Behaviour is otherwise identical: same checks, error messages, returned
   frame and backtest results. `ExecutionConfig` keeps its fields and defaults.
5. The existing tests keep their names and what they assert; only the way they
   call the function changes. Do not skip or delete tests.

Only Python sources change; leave the plan documents under `docs/` alone. Keep
the change mechanical. Do not run the test suite: held-out checks run after
your response, on Python 3.9 with pandas 2.0.1, so the code you touch must not
use newer syntax or library calls.
