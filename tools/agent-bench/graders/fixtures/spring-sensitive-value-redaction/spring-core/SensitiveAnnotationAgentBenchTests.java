package org.springframework.core.annotation;

import java.lang.annotation.Annotation;
import java.lang.annotation.Documented;
import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import java.lang.reflect.Method;
import java.util.Arrays;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveAnnotationAgentBenchTests {

	@Test
	void exposesRuntimeDocumentedMetaAnnotatableMarkerContract() {
		assertThat(Sensitive.class.isAnnotationPresent(Documented.class)).isTrue();
		assertThat(Sensitive.class.getAnnotation(Retention.class).value()).isEqualTo(RetentionPolicy.RUNTIME);
		assertThat(Arrays.asList(Sensitive.class.getAnnotation(Target.class).value()))
				.contains(ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER,
						ElementType.RECORD_COMPONENT, ElementType.ANNOTATION_TYPE);
	}

	@Test
	void detectsFieldsAccessorsParametersRecordComponentsAndComposedAnnotations() throws Exception {
		Method method = Sample.class.getDeclaredMethod("setPassword", String.class);
		assertThat(Sample.class.getDeclaredField("password").isAnnotationPresent(Sensitive.class)).isTrue();
		assertThat(method.isAnnotationPresent(Sensitive.class)).isTrue();
		assertThat(method.getParameters()[0].isAnnotationPresent(Sensitive.class)).isTrue();
		assertThat(Credentials.class.getRecordComponents()[0].isAnnotationPresent(Sensitive.class)).isTrue();
		assertThat(AnnotatedElementUtils.hasAnnotation(ComposedSensitive.class, Sensitive.class)).isTrue();
	}

	static class Sample {

		@Sensitive
		private String password;

		@Sensitive
		void setPassword(@Sensitive String password) {
			this.password = password;
		}
	}

	record Credentials(@Sensitive String password) {
	}

	@Target({ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER, ElementType.RECORD_COMPONENT})
	@Retention(RetentionPolicy.RUNTIME)
	@Sensitive
	@interface ComposedSensitive {
	}
}
