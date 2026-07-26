package org.springframework.validation;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

import org.junit.jupiter.api.Test;

import org.springframework.beans.MutablePropertyValues;
import org.springframework.beans.PropertyAccessException;
import org.springframework.core.ResolvableType;
import org.springframework.core.annotation.Sensitive;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveDataBinderAgentBenchTests {

	@Test
	void redactsValidatorRejectedRecordComponentWithoutMutatingTarget() {
		Credentials target = new Credentials("spring", "s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.addValidators(Validator.forInstanceOf(Credentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
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
		binder.addValidators(Validator.forInstanceOf(PlainCredentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue()).isEqualTo("visible");
	}

	@Test
	void redactsSubmittedSecretForTypeMismatchAndPreservesErrorMetadata() {
		NumericCredentials target = new NumericCredentials();
		DataBinder binder = new DataBinder(target, "credentials");
		binder.bind(new MutablePropertyValues().add("password", "s3cr3t"));

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
		directBinder.addValidators(Validator.forInstanceOf(DirectCredentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		directBinder.validate();
		assertThat(directBinder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("[REDACTED]");

		Profile profile = new Profile();
		profile.accounts.add(new Account("s3cr3t"));
		DataBinder nestedBinder = new DataBinder(profile, "profile");
		nestedBinder.addValidators(Validator.forInstanceOf(Profile.class,
				(object, errors) -> errors.rejectValue("accounts[0].password", "weak")));
		nestedBinder.validate();
		assertThat(nestedBinder.getBindingResult().getFieldError("accounts[0].password").getRejectedValue())
				.isEqualTo("[REDACTED]");
		assertThat(profile.accounts.get(0).getPassword()).isEqualTo("s3cr3t");
	}

	@Test
	void redactsConstructorBoundRecordComponent() {
		String submittedPassword = "s3cr3t";
		DataBinder binder = new DataBinder(null, "credentials");
		binder.setTargetType(ResolvableType.forClass(ConstructorCredentials.class));
		binder.construct(new DataBinder.ValueResolver() {
			@Override
			public Object resolveValue(String name, Class<?> type) {
				return (name.equals("username") ? "spring" : submittedPassword);
			}

			@Override
			public Set<String> getNames() {
				return Set.of("username", "password");
			}
		});

		FieldError error = binder.getBindingResult().getFieldError("password");
		assertThat(error).isNotNull();
		assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(submittedPassword).isEqualTo("s3cr3t");
	}

	@Test
	void detectsSensitiveAccessorsAndComposedAnnotationsDuringBinding() {
		AccessorCredentials accessorTarget = new AccessorCredentials();
		accessorTarget.setPassword("s3cr3t");
		DataBinder accessorBinder = new DataBinder(accessorTarget, "credentials");
		accessorBinder.addValidators(Validator.forInstanceOf(AccessorCredentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		accessorBinder.validate();
		assertThat(accessorBinder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("[REDACTED]");

		ComposedCredentials composedTarget = new ComposedCredentials();
		composedTarget.setPassword("s3cr3t");
		DataBinder composedBinder = new DataBinder(composedTarget, "credentials");
		composedBinder.addValidators(Validator.forInstanceOf(ComposedCredentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		composedBinder.validate();
		assertThat(composedBinder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("[REDACTED]");
	}

	@Test
	void supportsCustomDetectorForUnmarkedProperty() {
		PlainCredentials target = new PlainCredentials();
		target.setPassword("s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.setSensitiveValueDetector(context -> context.getPropertyPath().equals("password"));
		binder.addValidators(Validator.forInstanceOf(PlainCredentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue()).isEqualTo("[REDACTED]");
	}

	@Test
	void supportsCustomRedactorForSensitiveProperty() {
		Credentials target = new Credentials("spring", "s3cr3t");
		DataBinder binder = new DataBinder(target, "credentials");
		binder.setSensitiveValueRedactor((context, rejectedValue) ->
				"<hidden:" + context.getObjectName() + "." + context.getPropertyPath() + ">");
		binder.addValidators(Validator.forInstanceOf(Credentials.class,
				(object, errors) -> errors.rejectValue("password", "weak")));
		binder.validate();

		assertThat(binder.getBindingResult().getFieldError("password").getRejectedValue())
				.isEqualTo("<hidden:credentials.password>");
	}

	record Credentials(String username, @Sensitive String password) {
	}

	record ConstructorCredentials(String username, @Sensitive Integer password) {
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

	static class AccessorCredentials {

		private String password;

		@Sensitive
		public String getPassword() {
			return this.password;
		}

		public void setPassword(String password) {
			this.password = password;
		}
	}

	static class ComposedCredentials {

		@ComposedSensitive
		private String password;

		public String getPassword() {
			return this.password;
		}

		public void setPassword(String password) {
			this.password = password;
		}
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

	@Target({ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER, ElementType.RECORD_COMPONENT})
	@Retention(RetentionPolicy.RUNTIME)
	@Sensitive
	@interface ComposedSensitive {
	}
}
