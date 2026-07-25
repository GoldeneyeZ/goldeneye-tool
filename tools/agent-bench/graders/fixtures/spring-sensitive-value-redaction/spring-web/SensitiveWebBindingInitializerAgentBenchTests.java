package org.springframework.web.bind.support;

import org.junit.jupiter.api.Test;

import org.springframework.core.annotation.Sensitive;
import org.springframework.validation.DataBinder;
import org.springframework.validation.FieldError;
import org.springframework.web.bind.WebDataBinder;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveWebBindingInitializerAgentBenchTests {

	@Test
	void propagatesCustomRedactorToEveryWebDataBinder() {
		Credentials target = new Credentials();
		target.setPassword("s3cr3t");
		ConfigurableWebBindingInitializer initializer = new ConfigurableWebBindingInitializer();
		initializer.setSensitiveValueRedactor((context, rejectedValue) ->
				"<hidden:" + context.getObjectName() + "." + context.getPropertyPath() + ">");
		WebDataBinder binder = new WebDataBinder(target, "credentials");
		initializer.initBinder(binder);
		binder.addValidators((object, errors) -> errors.rejectValue("password", "weak"));
		binder.validate();

		FieldError error = binder.getBindingResult().getFieldError("password");
		assertThat(error.getRejectedValue()).isEqualTo("<hidden:credentials.password>");
		assertThat(target.getPassword()).isEqualTo("s3cr3t");
	}

	static class Credentials {

		@Sensitive
		private String password;

		public String getPassword() {
			return this.password;
		}

		public void setPassword(String password) {
			this.password = password;
		}
	}
}
