We are considering a contract change in Spring Framework's resource loading.
`org.springframework.core.io.support.ResourcePatternResolver.getResources(String)`
would stop declaring the checked `java.io.IOException`; implementations would
report I/O failures with the unchecked `java.io.UncheckedIOException` instead.

Before anyone touches it I need a written impact analysis. Limit it to the
`spring-core`, `spring-beans` and `spring-context` modules (`src/main/java`
and `src/test/java`); ignore the rest of the repository. Use the real class
and method names from the source, written as `Class.method`. Only this
interface method counts: other methods that merely share its name are not
part of the change and must not be listed as implementations, callers or
tests of it.

Cover, in this order:

1. Implementations. Every class in these modules that declares its own
   `getResources(String)` for this interface. For each one say whether it does
   the resolution itself, hands the call to another resolver (which one, and
   where that resolver comes from), or calls `super`, and under which
   condition.
2. Direct production callers. Every method that calls this interface method,
   including calls that an implementation makes on itself or on another
   resolver.
3. Dependence on the checked exception. For each caller, what happens today to
   an `IOException` thrown by the call: is it caught, and is it then translated
   (into which exception type), swallowed, or left to propagate. Where one
   class treats it differently on two code paths, describe both paths.
4. Tests. Which test classes call the method directly, and which existing
   assertion depends on the type of exception it throws (name the test class
   and the exception type it expects).

Do not modify any file. Put the complete answer in the `summary` field of your
final response.
