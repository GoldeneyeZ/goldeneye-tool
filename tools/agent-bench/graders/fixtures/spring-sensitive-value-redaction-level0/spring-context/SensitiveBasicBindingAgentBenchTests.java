package org.springframework.validation;

import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import java.util.ArrayList;
import java.util.List;

import org.junit.jupiter.api.Test;

import org.springframework.beans.MutablePropertyValues;
import org.springframework.core.annotation.Sensitive;

import static java.lang.annotation.ElementType.FIELD;
import static java.lang.annotation.ElementType.METHOD;
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
	void redactsNestedBeanPropertyTypeMismatch() {
		NestedRoot target = new NestedRoot();
		DataBinder binder = new DataBinder(target, "nestedRoot");
		MutablePropertyValues values = new MutablePropertyValues();
		values.add("credentials.password", "not-a-number");

		binder.bind(values);

		FieldError error = binder.getBindingResult().getFieldError("credentials.password");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(error.isBindingFailure()).isTrue();
		assertThat(target.getCredentials().getPassword()).isZero();
	}

	@Test
	void redactsIndexedBeanPropertyTypeMismatch() {
		IndexedRoot target = new IndexedRoot();
		DataBinder binder = new DataBinder(target, "indexedRoot");
		MutablePropertyValues values = new MutablePropertyValues();
		values.add("accounts[0].pin", "not-a-number");

		binder.bind(values);

		FieldError error = binder.getBindingResult().getFieldError("accounts[0].pin");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(error.isBindingFailure()).isTrue();
		assertThat(target.getAccounts().get(0).getPin()).isZero();
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

	static class NestedRoot {

		private final NestedCredentials credentials = new NestedCredentials();

		public NestedCredentials getCredentials() {
			return this.credentials;
		}
	}

	static class NestedCredentials {

		private int password;

		@Confidential
		public int getPassword() {
			return this.password;
		}

		public void setPassword(int password) {
			this.password = password;
		}
	}

	static class IndexedRoot {

		private final List<IndexedAccount> accounts = new ArrayList<>(List.of(new IndexedAccount()));

		public List<IndexedAccount> getAccounts() {
			return this.accounts;
		}
	}

	static class IndexedAccount {

		private int pin;

		@Sensitive
		public int getPin() {
			return this.pin;
		}

		public void setPin(int pin) {
			this.pin = pin;
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

	@Target({FIELD, METHOD})
	@Retention(RetentionPolicy.RUNTIME)
	@Sensitive
	@interface Confidential {
	}
}
