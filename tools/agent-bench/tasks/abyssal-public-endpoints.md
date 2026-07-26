Centralize the public-endpoint policy used by Spring Security and the JWT filter.

Requirements:

1. Add `fr.goldeneyetools.abyssalzenith.security.PublicEndpointPaths` as a non-instantiable utility class.
2. Expose `public static String[] patterns()`. Return a defensive copy containing exactly these Spring request-matcher patterns:
   - `/api/auth/**`
   - `/swagger-ui.html`
   - `/swagger-ui/**`
   - `/v3/api-docs/**`
   - `/v3/api-docs.yaml`
   - `/error`
   - `/api/weekly-rotations/active`
3. Expose `public static boolean matches(String servletPath)`. Return `false` for `null`. Interpret a trailing `/**` as matching its base path and slash-delimited descendants. Exact patterns must remain exact; do not match lookalike prefixes such as `/api/authentication` or `/swagger-ui-custom`.
4. Make `SecurityConfig` use `PublicEndpointPaths.patterns()` for its `permitAll` request matchers instead of maintaining a duplicate list.
5. Make `JwtAuthenticationFilter.shouldNotFilter` delegate to `PublicEndpointPaths.matches(...)` instead of maintaining its own duplicate list or matching loop.
6. Add focused Java tests covering exact paths, descendants, boundary lookalikes, `null`, and defensive-copy behavior.

Run the focused Maven tests. Keep the change limited to public-endpoint policy and tests.

