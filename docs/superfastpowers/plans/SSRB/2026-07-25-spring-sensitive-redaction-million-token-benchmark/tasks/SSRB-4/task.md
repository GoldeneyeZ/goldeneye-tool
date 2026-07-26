## Task 4: Add Level-2 Spring task and held-out grader

<TASK-ID>SSRB-4</TASK-ID>

**Files:**
- Create: `tools/agent-bench/tasks/spring-sensitive-value-redaction-level2.md`
- Create: `tools/agent-bench/graders/spring-sensitive-value-redaction.ps1`
- Create: `tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-core/SensitiveAnnotationAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveDataBinderAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveMethodValidationAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-web/SensitiveWebBindingInitializerAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webmvc/SensitiveMvcAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webflux/SensitiveWebFluxAgentBenchTests.java`

- [ ] **Step 1: Write grader contract tests**

The PowerShell test must create a temporary git repository shaped like the six
Spring modules, install fake `gradlew.bat`, execute the grader, and assert:

```powershell
It "copies every held-out fixture and removes it after PASS" {
	$result = Invoke-GraderFixture -GradleExitCode 0
	$result.ExitCode | Should -Be 0
	$result.RemainingAgentBenchFiles | Should -Be @()
	$result.GradleArguments | Should -Contain ":spring-context:test"
	$result.GradleArguments | Should -Contain ":spring-webmvc:test"
	$result.GradleArguments | Should -Contain ":spring-webflux:test"
}

It "fails before Gradle when a candidate path is outside policy" {
	$result = Invoke-GraderFixture -ExtraDirtyPath "settings.gradle"
	$result.ExitCode | Should -Not -Be 0
	$result.Output | Should -Match "Protocol violation"
}

It "removes held-out fixtures after Gradle failure" {
	$result = Invoke-GraderFixture -GradleExitCode 1
	$result.ExitCode | Should -Not -Be 0
	$result.RemainingAgentBenchFiles | Should -Be @()
}
```

- [ ] **Step 2: Run grader tests and verify failure**

Run:

```powershell
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: FAIL because grader and fixtures do not exist.

- [ ] **Step 3: Write the agent-visible Level-2 prompt**

The task must state exact behavior from the design spec, including:

```markdown
Implement opt-in sensitive-value redaction across Spring binding and method
validation.

Required public contracts:
- runtime, documented, meta-annotatable `org.springframework.core.annotation.Sensitive`;
- pluggable detector and redactor contracts in Spring validation;
- `DataBinder` detector/redactor configuration;
- `ConfigurableWebBindingInitializer` detector/redactor configuration.

Default behavior:
- annotation-based detection;
- replacement `[REDACTED]`;
- unmarked values unchanged.

Cover bean/direct-field access, conversion failures, validator rejection,
constructor binding, nested/indexed paths, method arguments, MVC, and WebFlux.
Redact error representations only. Never mutate target values or invocation
arguments. Preserve message codes, error arguments, binding-failure flags,
container indexes/keys, and source unwrapping.

Add focused production tests and run relevant module tests. Do not run `clean`.
Do not change build scripts, dependency declarations, generated files, or files
outside spring-core, spring-beans, spring-context, spring-web, spring-webmvc,
and spring-webflux.
```

Do not list expected implementation files.

- [ ] **Step 4: Add hidden annotation and DataBinder fixtures**

`SensitiveAnnotationAgentBenchTests` must assert runtime retention, field,
method, parameter, record-component, annotation-type targets, and composed
annotation discovery.

`SensitiveDataBinderAgentBenchTests` must include:

```java
record Credentials(String username, @Sensitive String password) {
}

@Test
void redactsValidatorRejectedRecordComponentWithoutMutatingTarget() {
	Credentials target = new Credentials("spring", "s3cr3t");
	DataBinder binder = new DataBinder(target, "credentials");
	binder.addValidators((object, errors) ->
			errors.rejectValue("password", "weak"));
	binder.validate();

	FieldError error = binder.getBindingResult().getFieldError("password");
	assertThat(error).isNotNull();
	assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
	assertThat(target.password()).isEqualTo("s3cr3t");
	assertThat(error.getCode()).isEqualTo("weak");
}
```

Additional complete test methods must assert:

- unmarked rejected value remains original;
- type mismatch redacts submitted secret;
- direct field access redacts;
- nested `accounts[0].password` redacts;
- custom detector marks an unannotated property;
- custom redactor returns `"<hidden:credentials.password>"`;
- `FieldError.isBindingFailure()`, codes, arguments, and source unwrap survive.

- [ ] **Step 5: Add method-validation fixture**

Create a service method with `@Sensitive @Size(min = 12) String token`, validate
through `MethodValidationAdapter`, and assert:

```java
ParameterValidationResult result =
		validationResult.getParameterValidationResults().get(0);
assertThat(result.getArgument()).isEqualTo("[REDACTED]");
assertThat(originalArguments[0]).isEqualTo("short");
assertThat(result.getResolvableErrors()).hasSize(1);
```

Also assert an unmarked parameter remains unchanged and source unwrapping still
returns the underlying `ConstraintViolation`.

- [ ] **Step 6: Add web initializer, MVC, and WebFlux fixtures**

The initializer test must configure a custom redactor, invoke
`initBinder(WebDataBinder)`, trigger a rejection, and observe the custom marker.

MVC and WebFlux tests must use their existing test infrastructure to submit a
secret to annotated model/request objects, obtain the resulting
`BindingResult` or validation exception, and assert:

```java
assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
assertThat(fieldError.toString()).doesNotContain("s3cr3t");
assertThat(exception.toString()).doesNotContain("s3cr3t");
```

Each test also asserts the controller target or captured invocation argument
still contains `s3cr3t`.

- [ ] **Step 7: Implement grader fixture installation and cleanup**

Use a manifest:

```powershell
$Fixtures = @(
	@{ Module = "spring-core"; Source = "spring-core/SensitiveAnnotationAgentBenchTests.java";
		Target = "spring-core/src/test/java/org/springframework/core/annotation/SensitiveAnnotationAgentBenchTests.java";
		Test = "org.springframework.core.annotation.SensitiveAnnotationAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveDataBinderAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/SensitiveDataBinderAgentBenchTests.java";
		Test = "org.springframework.validation.SensitiveDataBinderAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveMethodValidationAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/beanvalidation/SensitiveMethodValidationAgentBenchTests.java";
		Test = "org.springframework.validation.beanvalidation.SensitiveMethodValidationAgentBenchTests" },
	@{ Module = "spring-web"; Source = "spring-web/SensitiveWebBindingInitializerAgentBenchTests.java";
		Target = "spring-web/src/test/java/org/springframework/web/bind/support/SensitiveWebBindingInitializerAgentBenchTests.java";
		Test = "org.springframework.web.bind.support.SensitiveWebBindingInitializerAgentBenchTests" },
	@{ Module = "spring-webmvc"; Source = "spring-webmvc/SensitiveMvcAgentBenchTests.java";
		Target = "spring-webmvc/src/test/java/org/springframework/web/servlet/mvc/method/annotation/SensitiveMvcAgentBenchTests.java";
		Test = "org.springframework.web.servlet.mvc.method.annotation.SensitiveMvcAgentBenchTests" },
	@{ Module = "spring-webflux"; Source = "spring-webflux/SensitiveWebFluxAgentBenchTests.java";
		Target = "spring-webflux/src/test/java/org/springframework/web/reactive/result/method/annotation/SensitiveWebFluxAgentBenchTests.java";
		Test = "org.springframework.web.reactive.result.method.annotation.SensitiveWebFluxAgentBenchTests" }
)
```

Copy with `Copy-Item -LiteralPath`. In `finally`, remove only manifest targets.
Run each module once with all its selectors and `--build-cache`. Fail on first
non-zero Gradle exit. Before and after fixture installation, enforce candidate
dirty paths through the same module policy as config.

- [ ] **Step 8: Run grader contract tests**

Run:

```powershell
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: PASS.

- [ ] **Step 9: Commit**

```powershell
git add -- tools/agent-bench/tasks/spring-sensitive-value-redaction-level2.md tools/agent-bench/graders/spring-sensitive-value-redaction.ps1 tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1 tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction
git commit -m "bench: add Spring sensitive redaction task"
```
