package org.springframework.web.reactive.result.method.annotation;

import org.junit.jupiter.api.Test;

import org.springframework.core.MethodParameter;
import org.springframework.core.ReactiveAdapterRegistry;
import org.springframework.core.annotation.Sensitive;
import org.springframework.http.MediaType;
import org.springframework.validation.FieldError;
import org.springframework.validation.Validator;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.validation.annotation.Validated;
import org.springframework.web.bind.support.ConfigurableWebBindingInitializer;
import org.springframework.web.bind.support.WebExchangeBindException;
import org.springframework.web.reactive.BindingContext;
import org.springframework.web.testfixture.http.server.reactive.MockServerHttpRequest;
import org.springframework.web.testfixture.method.ResolvableMethod;
import org.springframework.web.testfixture.server.MockServerWebExchange;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.catchThrowable;

class SensitiveWebFluxAgentBenchTests {

	@Test
	void redactsWebFluxBindingExceptionWithoutMutatingBoundTarget() {
		ConfigurableWebBindingInitializer initializer = new ConfigurableWebBindingInitializer();
		initializer.setValidator(Validator.forInstanceOf(Credentials.class,
				(target, errors) -> errors.rejectValue("password", "weak")));
		BindingContext bindingContext = new BindingContext(initializer);
		MethodParameter parameter = ResolvableMethod.on(getClass()).named("handle").build().arg(Credentials.class);
		ModelAttributeMethodArgumentResolver resolver =
				new ModelAttributeMethodArgumentResolver(ReactiveAdapterRegistry.getSharedInstance(), false);

		Throwable failure = catchThrowable(() -> resolver.resolveArgument(parameter, bindingContext,
				MockServerWebExchange.from(MockServerHttpRequest.post("/")
						.contentType(MediaType.APPLICATION_FORM_URLENCODED)
						.body("password=s3cr3t"))).block());
		assertThat(failure).isInstanceOf(WebExchangeBindException.class);
		WebExchangeBindException exception = (WebExchangeBindException) failure;
		FieldError fieldError = exception.getFieldError("password");
		assertThat(fieldError).isNotNull();
		assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(fieldError.toString()).doesNotContain("s3cr3t");
		assertThat(exception.toString()).doesNotContain("s3cr3t");
		assertThat(((Credentials) exception.getTarget()).getPassword()).isEqualTo("s3cr3t");
	}

	@SuppressWarnings("unused")
	void handle(@ModelAttribute @Validated Credentials credentials) {
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
