package org.springframework.validation;

import org.junit.jupiter.api.Test;

import org.springframework.beans.MutablePropertyValues;
import org.springframework.core.annotation.Sensitive;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveBasicBindingAgentBenchTests {

	@Test
	void redactsBeanPropertyTypeMismatchWithoutMutatingTarget() {
		SecretNumber target = new SecretNumber();
		DataBinder binder = new DataBinder(target, "secretNumber");
		MutablePropertyValues values = new MutablePropertyValues();
		values.add("pin", "not-a-number");

		binder.bind(values);

		FieldError error = binder.getBindingResult().getFieldError("pin");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(error.isBindingFailure()).isTrue();
		assertThat(target.getPin()).isZero();
	}

	@Test
	void redactsDirectFieldTypeMismatch() {
		DirectSecret target = new DirectSecret();
		DataBinder binder = new DataBinder(target, "directSecret");
		binder.initDirectFieldAccess();
		MutablePropertyValues values = new MutablePropertyValues();
		values.add("pin", "not-a-number");

		binder.bind(values);

		FieldError error = binder.getBindingResult().getFieldError("pin");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(target.pin).isZero();
	}

	@Test
	void redactsValidatorRejectionWithoutMutatingTarget() {
		Credentials target = new Credentials("original-secret");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.addValidators(Validator.forInstanceOf(Credentials.class,
				(value, errors) -> errors.rejectValue("password", "invalid.password", "invalid")));

		binder.validate();

		FieldError error = binder.getBindingResult().getFieldError("password");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(error.getCode()).isEqualTo("invalid.password");
		assertThat(target.getPassword()).isEqualTo("original-secret");
	}

	@Test
	void leavesUnmarkedRejectedValueVisible() {
		VisibleNumber target = new VisibleNumber();
		DataBinder binder = new DataBinder(target, "visibleNumber");
		MutablePropertyValues values = new MutablePropertyValues();
		values.add("count", "not-a-number");

		binder.bind(values);

		assertThat(binder.getBindingResult().getFieldError("count").getRejectedValue())
				.isEqualTo("not-a-number");
	}

	static class SecretNumber {

		private int pin;

		@Sensitive
		public int getPin() {
			return this.pin;
		}

		public void setPin(int pin) {
			this.pin = pin;
		}
	}

	static class DirectSecret {

		@Sensitive
		int pin;
	}

	static class Credentials {

		private final String password;

		Credentials(String password) {
			this.password = password;
		}

		@Sensitive
		public String getPassword() {
			return this.password;
		}
	}

	static class VisibleNumber {

		private int count;

		public int getCount() {
			return this.count;
		}

		public void setCount(int count) {
			this.count = count;
		}
	}
}
