package org.springframework.validation.beanvalidation;

import java.lang.reflect.Method;

import jakarta.validation.ConstraintViolation;
import jakarta.validation.Validation;
import jakarta.validation.constraints.Size;
import org.junit.jupiter.api.Test;

import org.springframework.core.annotation.Sensitive;
import org.springframework.validation.method.MethodValidationResult;
import org.springframework.validation.method.ParameterValidationResult;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveMethodValidationAgentBenchTests {

	private final MethodValidationAdapter adapter =
			new MethodValidationAdapter(() -> Validation.buildDefaultValidatorFactory().getValidator());

	@Test
	void redactsSensitiveMethodArgumentWithoutChangingInvocationArgument() throws Exception {
		Service target = new Service();
		Method method = Service.class.getDeclaredMethod("authenticate", String.class, String.class);
		Object[] originalArguments = {"short", "short"};

		MethodValidationResult validationResult =
				this.adapter.validateArguments(target, method, null, originalArguments, new Class<?>[0]);

		ParameterValidationResult result = validationResult.getParameterValidationResults().get(0);
		assertThat(result.getArgument()).isEqualTo("[REDACTED]");
		assertThat(originalArguments[0]).isEqualTo("short");
		assertThat(result.getResolvableErrors()).hasSize(1);
		ConstraintViolation<?> violation = result.unwrap(result.getResolvableErrors().get(0), ConstraintViolation.class);
		assertThat(violation.getInvalidValue()).isEqualTo("short");
	}

	@Test
	void leavesUnmarkedMethodArgumentUnchanged() throws Exception {
		Service target = new Service();
		Method method = Service.class.getDeclaredMethod("authenticate", String.class, String.class);
		Object[] originalArguments = {"sufficiently-long", "short"};

		MethodValidationResult validationResult =
				this.adapter.validateArguments(target, method, null, originalArguments, new Class<?>[0]);

		ParameterValidationResult result = validationResult.getParameterValidationResults().get(0);
		assertThat(result.getArgument()).isEqualTo("short");
		assertThat(originalArguments[1]).isEqualTo("short");
	}

	static class Service {

		public void authenticate(@Sensitive @Size(min = 12) String token, @Size(min = 12) String visible) {
		}
	}
}
