import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("./", import.meta.url);

test("Level 0 stays limited to core/context binding redaction", async () => {
  const [configText, grader, prompt, fixture] = await Promise.all([
    readFile(new URL("configs/spring-sensitive-value-redaction-level0.json", root), "utf8"),
    readFile(new URL("graders/spring-sensitive-value-redaction-level0.ps1", root), "utf8"),
    readFile(new URL("tasks/spring-sensitive-value-redaction-level0.md", root), "utf8"),
    readFile(new URL("graders/fixtures/spring-sensitive-value-redaction-level0/spring-context/SensitiveBasicBindingAgentBenchTests.java", root), "utf8"),
  ]);
  const config = JSON.parse(configText);

  assert.deepEqual(config.allowed_dirty_policy.required_prefixes, [
    "spring-core/src/main/java/",
    "spring-context/src/main/java/",
  ]);
  assert.doesNotMatch(config.engines[0].env.GOLDENEYE_INCLUDE_PATHS, /spring-web/);
  assert.match(grader, /spring-core/);
  assert.match(grader, /spring-context/);
  assert.doesNotMatch(grader, /spring-web|SensitiveValueDetector|SensitiveValueRedactor/);
  assert.match(prompt, /outside\s+Level 0/);
  assert.match(prompt, /bean-property/);
  assert.match(prompt, /direct-field/);
  assert.match(prompt, /nested\/indexed/);
  assert.match(prompt, /timeout.*600000/i);
  assert.match(fixture, /credentials\.password/);
  assert.match(fixture, /accounts\[0\]\.pin/);
  assert.doesNotMatch(fixture, /addValidators\(\(/);
  assert.doesNotMatch(fixture, /new MutablePropertyValues\([^)]*,/);
});
