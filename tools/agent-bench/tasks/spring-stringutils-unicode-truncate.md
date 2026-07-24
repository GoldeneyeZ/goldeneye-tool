Update Spring Framework `spring-core` method
`org.springframework.util.StringUtils.truncate(CharSequence, int)`.

When truncation is required, the UTF-16 prefix must never end between the high
and low surrogate of one valid surrogate pair. If `threshold` falls between
that pair, shorten the prefix by one UTF-16 code unit before appending the
existing truncation suffix.

Preserve:
- existing positive-threshold precondition and message;
- existing suffix;
- existing behavior when `length() <= threshold`;
- existing UTF-16 code-unit threshold semantics in all other cases;
- `CharSequence` support.

Add focused coverage to
`spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`.

Run:
`.\gradlew.bat :spring-core:test --tests org.springframework.util.StringUtilsTests --build-cache`

Do not run `clean`. Do not change public API or unrelated files.
