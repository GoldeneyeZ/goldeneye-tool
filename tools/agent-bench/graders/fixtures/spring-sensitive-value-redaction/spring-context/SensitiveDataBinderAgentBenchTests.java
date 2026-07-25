package org.springframework.validation;

import java.util.ArrayList;
import java.util.List;

import org.junit.jupiter.api.Test;

import org.springframework.beans.MutablePropertyValues;
import org.springframework.beans.PropertyAccessException;
import org.springframework.core.annotation.Sensitive;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveDataBinderAgentBenchTests {

	@Test
	void redactsValidatorRejectedRecordComponentWithoutMutatingTarget() {
		Credentials target = new Credentials("spring", "s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		binder.validate();

		FieldError error = binder.getBindingResult().getFieldError("password");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(target.password()).isEqualTo("s3cr3t");
		assertThat(error.getCode()).isEqualTo("weak");
	}

	@Test
	void leavesUnmarkedRejectedValueUnchanged() {
		PlainCredentials target = new PlainCredentials();
		target.setPassword("visible");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue()).isEqualTo("visible");
	}

	@Test
	void redactsSubmittedSecretForTypeMismatchAndPreservesErrorMetadata() {
		NumericCredentials target = new NumericCredentials();
		DataBinder binder = new DataBinder(target, "credentials");
		binder.bind(new MutablePropertyValues("password", "s3cr3t"));

		FieldError error = binder.getBindingResult().getFieldError("password");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(error.isBindingFailure()).isTrue();
		assertThat(error.getCode()).isEqualTo("typeMismatch");
		assertThat(error.getArguments()).isNotEmpty();
		assertThat(error.unwrap(PropertyAccessException.class)).isNotNull();
		assertThat(target.getPassword()).isNull();
	}

	@Test
	void redactsDirectFieldAccessAndNestedIndexedPaths() {
		DirectCredentials directTarget = new DirectCredentials();
		DataBinder directBinder = new DataBinder(directTarget, "credentials");
		directBinder.initDirectFieldAccess();
		directBinder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		directBinder.validate();
		assertThat(directBinder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("[REDACTED]");

		Profile profile = new Profile();
		profile.accounts.add(new Account("s3cr3t"));
		DataBinder nestedBinder = new DataBinder(profile, "profile");
		nestedBinder.addValidators((object, errors) -> errors.rejectValue("accounts[0].password", "weak"));
		nestedBinder.validate();
		assertThat(nestedBinder.getBindingResult().getFieldError("accounts[0].password").getRejectedValue())
				.isEqualTo("[REDACTED]");
		assertThat(profile.accounts.get(0).getPassword()).isEqualTo("s3cr3t");
	}

	@Test
	void supportsCustomDetectorForUnmarkedProperty() {
		PlainCredentials target = new PlainCredentials();
		target.setPassword("s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.setSensitiveValueDetector(context -> context.getPropertyPath().equals("password"));
		binder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue()).isEqualTo("[REDACTED]");
	}

	@Test
	void supportsCustomRedactorForSensitiveProperty() {
		Credentials target = new Credentials("spring", "s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.setSensitiveValueRedactor((context, rejectedValue) ->
				"<hidden:" + context.getObjectName() + "." + context.getPropertyPath() + ">");
		binder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("<hidden:credentials.password>");
	}

	record Credentials(String username, @Sensitive String password) {
	}

	static class PlainCredentials {

		private String password;

		public String getPassword() {
			return this.password;
		}

		public void setPassword(String password) {
			this.password = password;
		}
	}

	static class NumericCredentials {

		@Sensitive
		private Integer password;

		public Integer getPassword() {
			return this.password;
		}

		public void setPassword(Integer password) {
			this.password = password;
		}
	}

	static class DirectCredentials {

		@Sensitive
		String password;
	}

	static class Profile {

		final List<Account> accounts = new ArrayList<>();

		public List<Account> getAccounts() {
			return this.accounts;
		}
	}

	static class Account {

		@Sensitive
		private String password;

		Account(String password) {
			this.password = password;
		}

		public String getPassword() {
			return this.password;
		}
	}
}
