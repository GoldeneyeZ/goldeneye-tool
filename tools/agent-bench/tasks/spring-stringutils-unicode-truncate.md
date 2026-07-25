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

Do not reassign `threshold`: the existing precondition captures it in a lambda.
Use a separate local endpoint for any surrogate-boundary adjustment.

Add focused coverage to
`spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`.

Do not run Gradle. The held-out grader runs the focused Spring Core test after
your response. You may run quick source and diff checks only.

Do not run `clean`. Do not change public API or unrelated files.
