package org.springframework.web.servlet.mvc.method.annotation;

import org.junit.jupiter.api.Test;

import org.springframework.core.MethodParameter;
import org.springframework.core.annotation.Sensitive;
import org.springframework.validation.BindingResult;
import org.springframework.validation.FieldError;
import org.springframework.validation.Validator;
import org.springframework.validation.annotation.Validated;
import org.springframework.web.bind.MethodArgumentNotValidException;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.support.ConfigurableWebBindingInitializer;
import org.springframework.web.bind.support.WebDataBinderFactory;
import org.springframework.web.context.request.ServletWebRequest;
import org.springframework.web.method.support.ModelAndViewContainer;
import org.springframework.web.testfixture.servlet.MockHttpServletRequest;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.catchThrowable;

class SensitiveMvcAgentBenchTests {

	@Test
	void redactsMvcBindingResultWithoutMutatingControllerTarget() throws Exception {
		ConfigurableWebBindingInitializer initializer = new ConfigurableWebBindingInitializer();
		initializer.setValidator(Validator.forInstanceOf(Credentials.class,
				(target, errors) -> errors.rejectValue("password", "weak")));
		WebDataBinderFactory binderFactory = new ServletRequestDataBinderFactory(null, initializer);
		MockHttpServletRequest request = new MockHttpServletRequest();
		request.addParameter("password", "s3cr3t");
		MethodParameter parameter = new MethodParameter(
				getClass().getDeclaredMethod("handle", Credentials.class), 0);
		ServletModelAttributeMethodProcessor processor = new ServletModelAttributeMethodProcessor(false);

		Throwable failure = catchThrowable(() -> processor.resolveArgument(parameter,
				new ModelAndViewContainer(), new ServletWebRequest(request), binderFactory));
		assertThat(failure).isInstanceOf(MethodArgumentNotValidException.class);
		BindingResult errors = ((MethodArgumentNotValidException) failure).getBindingResult();
		FieldError fieldError = errors.getFieldError("password");
		assertThat(fieldError).isNotNull();
		assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(fieldError.toString()).doesNotContain("s3cr3t");
		assertThat(((Credentials) errors.getTarget()).getPassword()).isEqualTo("s3cr3t");
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
