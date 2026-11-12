Add a read-only "payments summary" endpoint to the ViV-Wallet backend
(`ViV-Wallet-api`).

`GET /api/v1/payments/summary`

- Optional query parameters `from` and `to`: ISO dates (`yyyy-MM-dd`), both
  inclusive, compared with the payment date. A missing bound means "no limit"
  on that side; with neither, all payments are summarised.
- Only company administrators may call it, like the other restricted payment
  endpoints.
- `200 OK` with a JSON body of exactly three properties:
  `count` (number of payments in the period), `totalViv` (sum of their viv
  amounts) and `totalAmount` (that total converted to euros), for example
  `{"count": 2, "totalViv": 50, "totalAmount": 250}`.
- No payment in the period: still `200 OK`, with `count` 0, `totalViv` 0 and
  `totalAmount` 0.
- `from` later than `to`: `400 Bad Request` with an empty body.

Implementation constraints:

- Keep the controller thin. Put the computation in a new `PaymentService`
  method `summarize(LocalDate from, LocalDate to)` (a `null` bound means no
  limit) that returns a new response DTO `PaymentsSummaryDto`, placed next to
  `PaymentDto` and written with the same conventions.
- `totalAmount` must use the payment domain's existing viv-to-euro conversion.
  Do not re-implement the conversion or hard-code the rate.
- The working tree contains uncommitted work in progress in this area. Build
  on the code as it is now; do not revert, rename back or rewrite that work.
- Do not change existing endpoints, the request/response format of existing
  DTOs, or the database schema.
- Add unit tests for `summarize` to the existing payment service test class
  (bounds inclusive, missing bounds, empty period).

Do not run Maven or the tests (dependencies are not available offline); a
held-out grader checks the result after your response. Keep the change focused.
