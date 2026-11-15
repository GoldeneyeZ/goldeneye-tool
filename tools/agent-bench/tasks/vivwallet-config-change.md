Make the Lynx activity labels configurable in the ViV-Wallet backend
(`ViV-Wallet-api`, Spring Boot 2.1, Java 11).

When the backend imports activities from Lynx, it only keeps the activities
whose status is `Held` and whose validity is `OK`, and it uses the same `OK`
validity to tell a successful cooptation from a failed one. Both labels are
hard-coded, in more than one class. The Lynx administrators are about to
rename these picklist values, and we do not want to ship a new version each
time they do.

Required:

- Two new settings next to the existing `lynx.*` ones: `lynx.heldStatus`
  (default `Held`) and `lynx.okValidity` (default `OK`). With the defaults
  the import behaves exactly as it does today.
- Bind them the way the `x4b.auth` settings are bound: one configuration
  properties class, injected into the classes that need it. Do not read them
  with new `@Value` parameters or through the `Environment`.
- The defaults live in `application.yml`, like the other settings. Do not
  give the labels another value in any profile.
- A missing, empty or whitespace-only label must stop the application at
  startup instead of silently importing nothing. Enforce that on the
  properties class itself; Bean Validation (`javax.validation`) is already on
  the classpath through the web starter.
- Every place in the main code that decides on one of these labels must read
  the setting. None may still compare with a literal or with a constant.
- Leave the existing `lynx.vivApiUrl` and `lynx.userApiUrl` settings working.
  The Dockerfile and the Helm chart need no change.
- Update the existing tests so that they still compile, and add a test to
  the Lynx connector tests that configures labels other than the defaults
  and shows that an activity carrying them is imported.

Do not run Maven or the tests (dependencies are not available offline); a
held-out grader checks the result after your response. Keep the change focused.
