package org.springframework.web.servlet.mvc.method.annotation;

import jakarta.validation.Valid;
import org.junit.jupiter.api.Test;

import org.springframework.core.annotation.Sensitive;
import org.springframework.test.web.servlet.MockMvc;
import org.springframework.validation.BindingResult;
import org.springframework.validation.FieldError;
import org.springframework.web.bind.WebDataBinder;
import org.springframework.web.bind.annotation.InitBinder;
import org.springframework.web.bind.annotation.ModelAttribute;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.ResponseBody;

import static org.assertj.core.api.Assertions.assertThat;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;
import static org.springframework.test.web.servlet.setup.MockMvcBuilders.standaloneSetup;

class SensitiveMvcAgentBenchTests {

	@Test
	void redactsMvcBindingResultWithoutMutatingControllerTarget() throws Exception {
		SensitiveController controller = new SensitiveController();
		MockMvc mockMvc = standaloneSetup(controller).build();

		mockMvc.perform(post("/credentials").param("password", "s3cr3t"))
				.andExpect(status().isOk());

		FieldError fieldError = controller.fieldError;
		assertThat(fieldError).isNotNull();
		assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
		assertThat(fieldError.toString()).doesNotContain("s3cr3t");
		assertThat(controller.target.getPassword()).isEqualTo("s3cr3t");
	}

	static class SensitiveController {

		Credentials target;

		FieldError fieldError;

		@InitBinder
		public void initializeBinder(WebDataBinder binder) {
			binder.addValidators((target, errors) -> errors.rejectValue("password", "weak"));
		}

		@PostMapping("/credentials")
		@ResponseBody
		public String bind(@Valid @ModelAttribute Credentials credentials, BindingResult errors) {
			this.target = credentials;
			this.fieldError = errors.getFieldError("password");
			return "ok";
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
