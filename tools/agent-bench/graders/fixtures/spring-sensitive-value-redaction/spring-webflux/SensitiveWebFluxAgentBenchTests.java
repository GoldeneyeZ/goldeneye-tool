package org.springframework.web.reactive.result.method.annotation;

import jakarta.validation.Valid;
import org.junit.jupiter.api.Test;

import org.springframework.core.annotation.Sensitive;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.test.web.reactive.server.WebTestClient;
import org.springframework.validation.FieldError;
import org.springframework.web.bind.WebDataBinder;
import org.springframework.web.bind.annotation.ControllerAdvice;
import org.springframework.web.bind.annotation.ExceptionHandler;
import org.springframework.web.bind.annotation.InitBinder;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.support.WebExchangeBindException;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveWebFluxAgentBenchTests {

	@Test
	void redactsWebFluxBindingExceptionWithoutMutatingBoundTarget() {
		SensitiveController controller = new SensitiveController();
		ErrorCapture advice = new ErrorCapture();
		WebTestClient client = WebTestClient.bindToController(controller).controllerAdvice(advice).build();

		client.post().uri("/credentials")
				.contentType(MediaType.APPLICATION_FORM_URLENCODED)
				.bodyValue("password=s3cr3t")
				.exchange()
				.expectStatus().isBadRequest();

		WebExchangeBindException exception = advice.exception;
		assertThat(exception).isNotNull();
		FieldError fieldError = exception.getFieldError("password");
		assertThat(fieldError).isNotNull();
		assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(fieldError.toString()).doesNotContain("s3cr3t");
		assertThat(exception.toString()).doesNotContain("s3cr3t");
		assertThat(((Credentials) exception.getTarget()).getPassword()).isEqualTo("s3cr3t");
	}

	static class SensitiveController {

		@InitBinder
		public void initializeBinder(WebDataBinder binder) {
			binder.addValidators((target, errors) -> errors.rejectValue("password", "weak"));
		}

		@PostMapping("/credentials")
		public void bind(@Valid @ModelAttribute Credentials credentials) {
		}
	}

	@ControllerAdvice
	static class ErrorCapture {

		WebExchangeBindException exception;

		@ExceptionHandler(WebExchangeBindException.class)
		@ResponseStatus(HttpStatus.BAD_REQUEST)
		public void handle(WebExchangeBindException exception) {
			this.exception = exception;
		}
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
