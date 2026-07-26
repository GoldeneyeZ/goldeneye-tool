package org.springframework.core.annotation;

import java.lang.annotation.Documented;
import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class SensitiveBasicAnnotationAgentBenchTests {

	@Test
	void exposesRuntimeDocumentedFieldAndAccessorMarker() {
		Retention retention = Sensitive.class.getAnnotation(Retention.class);
		Target target = Sensitive.class.getAnnotation(Target.class);

		assertThat(retention).isNotNull();
		assertThat(retention.value()).isEqualTo(RetentionPolicy.RUNTIME);
		assertThat(Sensitive.class.isAnnotationPresent(Documented.class)).isTrue();
		assertThat(target.value()).contains(
				ElementType.FIELD, ElementType.METHOD, ElementType.ANNOTATION_TYPE);
	}
}
