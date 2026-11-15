#!/usr/bin/env node
// Grader for the `vivwallet-config-change` task (Lynx activity labels as
// configuration: lynx.heldStatus / lynx.okValidity).
//
//   node vivwallet-config-change.mjs <worktree> [<source repo>]
//
// The project's Maven dependencies (Spring Boot 2.1.3) cannot be resolved
// offline, so nothing is compiled or run. This grader is STRUCTURAL, but it
// does not grep: fixtures/vivwallet-config-change.JavaFacts.java parses every
// Java file under ViV-Wallet-api/src with the JDK compiler (parse only, Java
// 11) and prints the declarations, string literals, references, constructor
// calls and method calls. From those facts and the YAML files it checks that
//   1. every Java file still parses at Java 11, with no jakarta.* import;
//   2. one bean-registered @ConfigurationProperties class is bound to the
//      prefix `lynx` and declares the String properties heldStatus and
//      okValidity in a way Spring Boot 2.1 can bind (setters);
//   3. that class rejects a blank label at startup (@Validated + @NotBlank,
//      or a blank check that fails in a setter / @PostConstruct / validator);
//   4. application.yml carries both keys with the defaults `Held` and `OK`,
//      once, next to the untouched lynx.vivApiUrl / lynx.userApiUrl, and no
//      main resource gives them another value;
//   5. no main class other than the properties class decides with a
//      hard-coded `Held` / `OK` (a literal, or a constant holding it), reads
//      the keys through @Value / Environment, or has lost the comparison;
//   6. the properties class is injected somewhere and imported where used;
//   7. every `new LynxConnector(..)`, `new ActivityToActionMapper(..)` and
//      `new <properties class>(..)` in main and test code matches a declared
//      constructor (the existing tests were updated);
//   8. the existing Lynx connector tests are still there and some test
//      configures a label other than the defaults.
//
// Exit 0 = pass. Failures are printed to stderr.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const worktree = resolve(process.argv[2] ?? ".");
const here = dirname(fileURLToPath(import.meta.url));
const factsProgram = join(here, "fixtures", "vivwallet-config-change.JavaFacts.java");

const SRC = join(worktree, "ViV-Wallet-api", "src");
const MAIN_RESOURCES = join(SRC, "main", "resources");
const TEST_RESOURCES = join(SRC, "test", "resources");
const PACKAGE_DIR = "java/com/invivoo/vivwallet/api";
const CONNECTOR = `main/${PACKAGE_DIR}/infrastructure/lynx/LynxConnector.java`;
const MAPPER = `main/${PACKAGE_DIR}/infrastructure/lynx/mapper/ActivityToActionMapper.java`;
const CONNECTOR_TEST = `test/${PACKAGE_DIR}/infrastructure/lynx/LynxConnectorTest.java`;
const LYNX_PACKAGE = `main/${PACKAGE_DIR}/infrastructure/lynx/`;

const SETTINGS = [
  { property: "heldStatus", key: "lynx.heldStatus", fallback: "Held" },
  { property: "okValidity", key: "lynx.okValidity", fallback: "OK" },
];
const EXISTING_CONNECTOR_TESTS = [
  "should_return_activities_when_get_from_lynx",
  "should_get_2_actions_with_10_and_40_viv_for_a_winning_coaching_done_by_a_single_collaborator",
  "should_get_2_actions_with_10_and_20_viv_by_collaborator_for_a_winning_coaching_done_by_2_collaborators",
  "should_get_all_lynx_users",
];

const failures = [];
const fail = (message) => {
  if (!failures.includes(message)) failures.push(message);
};

if (!existsSync(join(SRC, "main", "java", "com", "invivoo", "vivwallet", "api"))) {
  fail(`not a ViV-Wallet worktree: ${worktree}`);
  finish();
}

const facts = readFacts();
const files = facts.files;
const mainFiles = files.filter((file) => file.path.startsWith("main/"));
const testFiles = files.filter((file) => file.path.startsWith("test/"));
const byPath = new Map(files.map((file) => [file.path, file]));

checkSyntax();
const properties = findPropertiesClass();
if (properties) {
  checkBinding(properties);
  checkRegistration(properties);
  checkValidation(properties);
}
checkConfigurationFiles();
checkNoHardCodedLabel(properties);
checkComparisonsKept();
if (properties) {
  checkInjection(properties);
  checkConstructorCalls(properties);
}
checkTests(properties);
finish();

// ---------------------------------------------------------------- facts
function readFacts() {
  if (!existsSync(factsProgram)) {
    fail(`facts fixture missing: ${factsProgram}`);
    finish();
  }
  const run = spawnSync("java", [factsProgram, "11", SRC], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    timeout: 150_000,
    windowsHide: true,
  });
  if (run.error || run.status !== 0) {
    const reason = run.error ? run.error.message : `exit ${run.status}:\n${tail(`${run.stdout}\n${run.stderr}`)}`;
    fail(`could not read the Java sources (java 11+ required on PATH): ${reason}`);
    finish();
  }
  try {
    return JSON.parse(run.stdout);
  } catch (error) {
    fail(`unreadable output of the Java facts program: ${error.message}`);
    return finish();
  }
}

// ---------------------------------------------------------------- 1. syntax
function checkSyntax() {
  if (facts.errors.length > 0) {
    fail(`Java sources do not parse at Java 11:\n${facts.errors.slice(0, 20).join("\n")}`);
  }
  for (const file of files) {
    const jakarta = file.imports.find((name) => /^(static )?jakarta\./.test(name));
    if (jakarta) fail(`${file.path}: imports ${jakarta}, which is not on the classpath of this Spring Boot 2.1 project (javax.*)`);
  }
}

// ---------------------------------------------------------------- 2. properties class
function findPropertiesClass() {
  const candidates = [];
  for (const file of mainFiles) {
    for (const cls of file.classes) {
      const annotation = cls.annotations.find((item) => item.name === "ConfigurationProperties");
      if (annotation && prefixOf(annotation.text) === "lynx") {
        candidates.push({ file, cls, beanMethod: null });
      }
      for (const method of cls.methods) {
        const onMethod = method.annotations.find((item) => item.name === "ConfigurationProperties");
        if (!onMethod || prefixOf(onMethod.text) !== "lynx") continue;
        const returned = simpleType(method.returnType);
        for (const other of mainFiles) {
          const target = other.classes.find((item) => item.name === returned);
          if (target) candidates.push({ file: other, cls: target, beanMethod: method });
        }
      }
    }
  }
  if (candidates.length === 0) {
    fail("no @ConfigurationProperties class is bound to the prefix `lynx`: the two labels must be bound the way the x4b.auth settings are");
    return null;
  }
  const complete = candidates.find((candidate) =>
    SETTINGS.every((setting) => candidate.cls.fields.some((field) => field.name === setting.property)),
  );
  return complete ?? candidates[0];
}

function prefixOf(annotationText) {
  const open = annotationText.indexOf("(");
  if (open === -1) return null;
  const args = annotationText.slice(open + 1, annotationText.lastIndexOf(")"));
  const named = /\b(?:value|prefix)\s*=\s*([^,]+)/.exec(args);
  const expression = (named ? named[1] : /=/.test(args.replace(/"[^"]*"/g, "")) ? "" : args).trim();
  const literal = /^"([^"]*)"$/.exec(expression);
  if (literal) return literal[1];
  const constant = /^(?:\w+\s*\.\s*)*(\w+)$/.exec(expression);
  if (!constant) return null;
  const values = new Set();
  for (const file of mainFiles) {
    for (const variable of file.vars) if (variable.name === constant[1]) values.add(variable.value);
  }
  return values.size === 1 ? [...values][0] : null;
}

function checkBinding({ file, cls }) {
  const classSetters = hasAnnotation(cls.annotations, ["Setter", "Data"]);
  for (const setting of SETTINGS) {
    const field = cls.fields.find((item) => item.name === setting.property);
    if (!field) {
      fail(`${file.path}: ${cls.name} has no \`${setting.property}\` property, so ${setting.key} is not bound`);
      continue;
    }
    if (field.static) fail(`${file.path}: ${cls.name}.${setting.property} is static and cannot be bound`);
    if (simpleType(field.type) !== "String") {
      fail(`${file.path}: ${cls.name}.${setting.property} must be a String (found ${field.type})`);
    }
    const setterName = `set${capitalize(setting.property)}`;
    const setter = classSetters || hasAnnotation(field.annotations, ["Setter"]) ||
      cls.methods.some((method) => method.name === setterName && method.params === 1);
    if (!setter || field.final) {
      fail(`${file.path}: ${cls.name}.${setting.property} has no setter; Spring Boot 2.1 binds @ConfigurationProperties through setters`);
    }
    if (field.init !== null) {
      const value = field.initString ?? constantValue(file, field.init);
      if (value !== null && value !== setting.fallback) {
        fail(`${file.path}: the default of ${setting.key} must stay \`${setting.fallback}\` (found \`${value}\`)`);
      }
    }
  }
}

function constantValue(file, expression) {
  const name = /^(?:\w+\s*\.\s*)*(\w+)$/.exec(expression.trim());
  if (!name) return null;
  const local = file.vars.filter((variable) => variable.name === name[1]);
  if (local.length === 1) return local[0].value;
  const values = new Set();
  for (const other of mainFiles) {
    for (const variable of other.vars) if (variable.name === name[1]) values.add(variable.value);
  }
  return values.size === 1 ? [...values][0] : null;
}

function checkRegistration({ file, cls, beanMethod }) {
  const ways = [];
  if (hasAnnotation(cls.annotations, ["Component", "Configuration", "Service"])) ways.push("a stereotype annotation");
  for (const other of mainFiles) {
    for (const owner of other.classes) {
      const enable = owner.annotations.find((item) => item.name === "EnableConfigurationProperties");
      if (enable && new RegExp(`\\b${cls.name}\\s*\\.\\s*class\\b`).test(enable.text)) {
        ways.push(`@EnableConfigurationProperties on ${owner.name}`);
      }
      for (const method of owner.methods) {
        if (hasAnnotation(method.annotations, ["Bean"]) && simpleType(method.returnType) === cls.name) {
          ways.push(`@Bean ${owner.name}.${method.name}()`);
        }
      }
    }
  }
  if (ways.length === 0) {
    fail(`${file.path}: ${cls.name} is not registered as a bean (@Component like X4BAuthConfiguration, or @EnableConfigurationProperties)`);
  } else if (ways.length > 1) {
    fail(`${file.path}: ${cls.name} is registered more than once (${ways.join(" and ")}): two beans of that type make its injection ambiguous`);
  }
  if (beanMethod && !hasAnnotation(beanMethod.annotations, ["Bean"])) {
    fail(`${file.path}: the method carrying @ConfigurationProperties for ${cls.name} is not a @Bean method`);
  }
}

// ---------------------------------------------------------------- 3. validation
function checkValidation({ file, cls, beanMethod }) {
  const validated = hasAnnotation(cls.annotations, ["Validated"]) ||
    (beanMethod && hasAnnotation(beanMethod.annotations, ["Validated"]));
  const constrained = SETTINGS.map((setting) => constraintsOf(cls, setting.property));
  const declarative = validated && constrained.every((names) => names.some((name) => name === "NotBlank" || name === "Pattern"));
  const imperative = failsOnBlank(file, cls);
  if (declarative) {
    const constraintImport = file.imports.some((name) =>
      /^(javax\.validation\.constraints|org\.hibernate\.validator\.constraints)\.(\*|NotBlank|Pattern)$/.test(name));
    const inlineConstraint = cls.fields.some((field) =>
      field.annotations.some((item) => /@(javax\.validation|org\.hibernate\.validator)\./.test(item.text)));
    if (!constraintImport && !inlineConstraint) {
      fail(`${file.path}: the constraint annotations are not imported from javax.validation.constraints`);
    }
    const validatedImport = file.imports.some((name) =>
      /^org\.springframework\.validation\.annotation\.(\*|Validated)$/.test(name)) ||
      [...cls.annotations, ...(beanMethod ? beanMethod.annotations : [])].some((item) =>
        /@org\.springframework\.validation\.annotation\.Validated/.test(item.text));
    if (!validatedImport) {
      fail(`${file.path}: @Validated must be org.springframework.validation.annotation.Validated`);
    }
    return;
  }
  if (imperative) return;
  const weak = constrained.flat().filter((name) => ["NotNull", "NotEmpty", "Size"].includes(name));
  if (!validated && constrained.flat().length > 0) {
    fail(`${file.path}: ${cls.name} has constraint annotations but no @Validated, so Spring Boot never checks them`);
  } else if (validated && weak.length > 0) {
    fail(`${file.path}: @${weak[0]} lets a blank label through; a missing or blank label must stop the startup (@NotBlank on both properties)`);
  } else {
    fail(`${file.path}: ${cls.name} does not reject a missing or blank label at startup (expected @Validated with @NotBlank on heldStatus and okValidity, or an equivalent check in the class)`);
  }
}

function constraintsOf(cls, property) {
  const names = [];
  const field = cls.fields.find((item) => item.name === property);
  if (field) names.push(...field.annotations.map((item) => item.name));
  const suffix = capitalize(property);
  for (const method of cls.methods) {
    if (method.name === `get${suffix}` || method.name === `set${suffix}`) {
      names.push(...method.annotations.map((item) => item.name), ...method.paramAnnotations.map((item) => item.name));
    }
  }
  return names;
}

// A blank check that fails, reached from a setter, @PostConstruct,
// afterPropertiesSet or a Validator.validate implementation of the class.
function failsOnBlank(file, cls) {
  const entry = (name) => name !== null && (/^set[A-Z]/.test(name) || name === "afterPropertiesSet" || name === "validate" ||
    cls.methods.some((method) => method.name === name && hasAnnotation(method.annotations, ["PostConstruct"])));
  const reached = (name) => entry(name) ||
    file.calls.some((call) => call.name === name && entry(call.encl));
  const blankChecks = file.calls.filter((call) =>
    /^(isBlank|isNotBlank|hasText|notBlank|isEmpty|isNotEmpty|hasLength|isWhitespace|trim|strip|rejectIfEmptyOrWhitespace)$/.test(call.name) &&
    reached(call.encl));
  if (blankChecks.length === 0) return false;
  const failing = file.throws > 0 || file.calls.some((call) =>
    /^(hasText|notBlank|rejectValue|reject|rejectIfEmptyOrWhitespace|state|isTrue)$/.test(call.name) &&
    /\b(Assert|Validate|ValidationUtils|[eE]rrors)\s*\.\s*\w+\s*\(/.test(call.text));
  return failing;
}

// ---------------------------------------------------------------- 4. configuration files
function checkConfigurationFiles() {
  const applicationYml = join(MAIN_RESOURCES, "application.yml");
  if (!existsSync(applicationYml)) {
    fail("ViV-Wallet-api/src/main/resources/application.yml is missing");
    return;
  }
  const text = readFileSync(applicationYml, "utf8");
  const entries = flattenYaml(text);
  for (const setting of SETTINGS) {
    const found = entries.filter((entry) => canonical(entry.key) === canonical(setting.key));
    if (found.length === 0) {
      fail(`application.yml does not declare ${setting.key} (the defaults live there, like the other settings)`);
    } else if (found.length > 1) {
      fail(`application.yml declares ${setting.key} ${found.length} times`);
    } else if (found[0].value !== setting.fallback) {
      fail(`application.yml: the default of ${setting.key} must be \`${setting.fallback}\` (found \`${found[0].value}\`)`);
    }
  }
  if (!/^---/m.test(text) && entries.filter((entry) => entry.key === "lynx").length > 1) {
    fail("application.yml has two top-level `lynx:` blocks; a duplicate key makes the YAML file unloadable");
  }
  for (const key of ["lynx.vivApiUrl", "lynx.userApiUrl", "app.url", "api.key", "x4b.auth.issuer"]) {
    if (!entries.some((entry) => entry.key === key)) fail(`application.yml lost the existing setting ${key}`);
  }
  for (const name of ["vivApiUrl", "userApiUrl"]) {
    const bound = mainFiles.some((file) =>
      file.literals.some((literal) => literal.ann && literal.value.includes(`lynx.${name}`)) ||
      file.classes.some((cls) => hasPrefixLynx(cls) && cls.fields.some((field) => field.name === name)));
    if (!bound) fail(`the existing setting lynx.${name} is no longer bound anywhere`);
  }
  // no other main resource may give the labels another value
  for (const name of listFiles(MAIN_RESOURCES).filter((file) => /\.(ya?ml|properties)$/.test(file))) {
    if (name === applicationYml) continue;
    const others = /\.properties$/.test(name) ? flattenProperties(readFileSync(name, "utf8")) : flattenYaml(readFileSync(name, "utf8"));
    for (const setting of SETTINGS) {
      for (const entry of others) {
        if (canonical(entry.key) === canonical(setting.key) && entry.value !== setting.fallback) {
          fail(`${name.slice(SRC.length + 1).replaceAll("\\", "/")}: gives ${setting.key} the value \`${entry.value}\`; the default must stay \`${setting.fallback}\` in every profile`);
        }
      }
    }
  }
}

function hasPrefixLynx(cls) {
  const annotation = cls.annotations.find((item) => item.name === "ConfigurationProperties");
  return Boolean(annotation) && prefixOf(annotation.text) === "lynx";
}

// ---------------------------------------------------------------- 5. hard-coded labels
function checkNoHardCodedLabel(properties) {
  const propertiesPath = properties ? properties.file.path : null;
  const isLabel = (value) => SETTINGS.some((setting) => value.trim().toLowerCase() === setting.fallback.toLowerCase());
  // a use of the constant `owner.constant` declared in `ownerPath`: a
  // qualified selection, or a plain identifier in the declaring file or in a
  // file that imports it statically
  const usesConstant = (file, owner, constant, ownerPath) => {
    if (file.selects.includes(`${owner}.${constant}`)) return true;
    if (file.path === ownerPath) return file.idents.includes(constant) || file.selects.includes(`this.${constant}`);
    return file.idents.includes(constant) && file.imports.some((name) =>
      name.startsWith("static ") && (name.endsWith(`.${owner}.${constant}`) || name.endsWith(`.${owner}.*`)));
  };
  const usedOutsideProperties = (owner, constant, ownerPath) => mainFiles.filter((file) =>
    file.path !== propertiesPath && usesConstant(file, owner, constant, ownerPath)).map((file) => file.path);

  for (const file of mainFiles) {
    if (file.path !== propertiesPath) {
      for (const call of file.calls) {
        const key = call.args.find((argument) => /lynx\.(held-?status|ok-?validity)/i.test(argument.str ?? ""));
        if (key && /^(getProperty|getRequiredProperty|containsProperty|resolvePlaceholders|resolveRequiredPlaceholders)$/.test(call.name)) {
          fail(`${file.path}: looks the key up by name (\`${key.str}\`); the labels must come from the properties class`);
        }
      }
    }
    for (const literal of file.literals) {
      if (literal.ann) {
        if (/lynx\.(held-?status|ok-?validity)/i.test(literal.value) && file.path !== propertiesPath) {
          fail(`${file.path}: reads \`${literal.value}\` through an annotation; the labels must come from the properties class, not from @Value`);
        }
        continue;
      }
      if (!isLabel(literal.value)) continue;
      const inProperties = file.path === propertiesPath;
      if (literal.field !== null && (literal.static || !inProperties)) {
        const users = usedOutsideProperties(literal.cls, literal.field, file.path);
        if (users.length > 0) {
          fail(`${users[0]}: still decides with the hard-coded constant ${literal.cls}.${literal.field} = "${literal.value}" instead of the setting`);
        }
      } else if (!inProperties) {
        fail(`${file.path}: still contains the hard-coded label "${literal.value}"; it must read the setting`);
      }
    }
  }
}

function checkComparisonsKept() {
  const mapper = byPath.get(MAPPER);
  const connector = byPath.get(CONNECTOR);
  if (!connector) fail(`${CONNECTOR} is missing`);
  if (!mapper) {
    fail(`${MAPPER} is missing`);
  } else {
    for (const type of ["COOPTATION", "COOPTATION_NOK"]) {
      if (!mapper.refs.includes(type)) fail(`${MAPPER}: no longer distinguishes a successful cooptation from a failed one (ActionType.${type})`);
    }
  }
  const scope = mainFiles.filter((file) => file.path.startsWith(LYNX_PACKAGE) && !file.path.includes("/model/") &&
    !file.path.endsWith("/LynxUserToUserMapper.java"));
  const names = new Set(scope.flatMap((file) => file.refs));
  for (const file of mainFiles) {
    if (file.classes.some(hasPrefixLynx)) file.refs.forEach((name) => names.add(name));
  }
  if (!names.has("getStatus") && !names.has("status")) {
    fail("the Lynx import no longer checks the status of an activity");
  }
  if (!names.has("getValidity") && !names.has("validity")) {
    fail("the Lynx import no longer checks the validity of an activity");
  }
  if (connector) {
    for (const method of ["findActions", "getActionsFromActivities", "findUsers"]) {
      if (!connector.classes.some((cls) => cls.methods.some((item) => item.name === method))) {
        fail(`${CONNECTOR}: the existing method ${method} was removed`);
      }
    }
  }
}

// ---------------------------------------------------------------- 6. injection
function checkInjection({ file, cls }) {
  const users = mainFiles.filter((other) => other.path !== file.path && other.refs.includes(cls.name));
  if (users.length === 0) {
    fail(`${cls.name} is declared but no main class uses it`);
  }
  for (const other of files) {
    if (other.path === file.path || !other.refs.includes(cls.name)) continue;
    if (other.package === file.package) continue;
    const imported = other.imports.some((name) => name.endsWith(`.${cls.name}`) || name === `${file.package}.*`) ||
      other.selects.some((name) => name.endsWith(`.${cls.name}`));
    if (!imported) fail(`${other.path}: uses ${cls.name} without importing ${file.package}.${cls.name}`);
  }
}

// ---------------------------------------------------------------- 7. constructor calls
function checkConstructorCalls(properties) {
  const targets = [properties.cls];
  for (const path of [CONNECTOR, MAPPER]) {
    const file = byPath.get(path);
    const name = path.slice(path.lastIndexOf("/") + 1, -".java".length);
    const cls = file?.classes.find((item) => item.name === name);
    if (cls) targets.push(cls);
  }
  for (const cls of targets) {
    const allowed = constructorArities(cls);
    for (const file of files) {
      for (const created of file.news) {
        if (created.type !== cls.name) continue;
        if (!allowed.has(created.args.length)) {
          fail(`${file.path}: \`new ${cls.name}(...)\` is called with ${created.args.length} argument(s) but ${cls.name} declares constructors with ${[...allowed].sort().join(" or ")}; this does not compile`);
        }
      }
    }
  }
}

function constructorArities(cls) {
  const arities = new Set(cls.ctors.map((ctor) => ctor.params));
  const instanceFields = cls.fields.filter((field) => !field.static);
  const required = instanceFields.filter((field) =>
    (field.final && field.init === null) || hasAnnotation(field.annotations, ["NonNull"]));
  if (hasAnnotation(cls.annotations, ["NoArgsConstructor"])) arities.add(0);
  if (hasAnnotation(cls.annotations, ["AllArgsConstructor"])) {
    arities.add(instanceFields.filter((field) => !(field.final && field.init !== null)).length);
  }
  if (hasAnnotation(cls.annotations, ["RequiredArgsConstructor"])) arities.add(required.length);
  const explicit = cls.ctors.length > 0 ||
    hasAnnotation(cls.annotations, ["NoArgsConstructor", "AllArgsConstructor", "RequiredArgsConstructor"]);
  if (!explicit) {
    if (hasAnnotation(cls.annotations, ["Data", "Value"])) arities.add(required.length);
    else if (hasAnnotation(cls.annotations, ["Builder"])) arities.add(instanceFields.length);
    else arities.add(0);
  }
  return arities;
}

// ---------------------------------------------------------------- 8. tests
function checkTests(properties) {
  const connectorTest = byPath.get(CONNECTOR_TEST);
  if (!connectorTest) {
    fail(`${CONNECTOR_TEST} was removed`);
  } else {
    const methods = connectorTest.classes.flatMap((cls) => cls.methods);
    for (const name of EXISTING_CONNECTOR_TESTS) {
      const method = methods.find((item) => item.name === name);
      if (!method || !hasAnnotation(method.annotations, ["Test"])) {
        fail(`${CONNECTOR_TEST}: the existing test ${name} was removed or disabled`);
      } else if (hasAnnotation(method.annotations, ["Ignore", "Disabled"])) {
        fail(`${CONNECTOR_TEST}: the existing test ${name} was disabled`);
      }
    }
  }
  if (!properties) return;

  const isDefault = (value) => SETTINGS.some((setting) => value.trim().toLowerCase() === setting.fallback.toLowerCase());
  const customLabels = new Set();
  const consider = (value) => {
    if (typeof value !== "string") return;
    const label = value.trim();
    if (label === "" || isDefault(label) || /^[a-z][a-z0-9+.-]*:\/\//i.test(label) || /^lynx\./i.test(label)) return;
    customLabels.add(label);
  };
  const accessor = /^(set|with)?(heldStatus|okValidity)$/i;
  for (const file of testFiles) {
    const resolveArgument = (argument, enclosing) => {
      if (argument.str !== null) return [argument.str];
      if (argument.ident === null) return [];
      const values = file.vars.filter((variable) => variable.name === argument.ident).map((variable) => variable.value);
      if (argument.param >= 0 && enclosing !== null) {
        // one level up: the literal passed by the callers of the helper method
        for (const call of file.calls) {
          if (call.name !== enclosing) continue;
          const passed = call.args[argument.param];
          if (!passed) continue;
          if (passed.str !== null) values.push(passed.str);
          else if (passed.ident !== null) {
            values.push(...file.vars.filter((variable) => variable.name === passed.ident).map((variable) => variable.value));
          }
        }
      }
      return values;
    };
    for (const created of file.news) {
      if (created.type !== properties.cls.name) continue;
      const positions = labelPositions(properties.cls, created.args.length);
      created.args.forEach((argument, index) => {
        if (positions === null || positions.includes(index)) resolveArgument(argument, created.encl).forEach(consider);
      });
    }
    for (const call of file.calls) {
      if (accessor.test(call.name)) {
        for (const argument of call.args) resolveArgument(argument, call.encl).forEach(consider);
      }
      if (/(thenReturn|willReturn|doReturn|thenAnswer)\s*\(/.test(call.text) && /(HeldStatus|OkValidity)\s*\(/i.test(call.text)) {
        for (const match of call.text.matchAll(/"((?:[^"\\]|\\.)*)"/g)) consider(match[1]);
        for (const argument of call.args) resolveArgument(argument, call.encl).forEach(consider);
      }
      if (call.name === "setField" && call.args.some((argument) => /^(heldStatus|okValidity)$/.test(argument.str ?? ""))) {
        for (const argument of call.args) {
          if (!/^(heldStatus|okValidity)$/.test(argument.str ?? "")) resolveArgument(argument, call.encl).forEach(consider);
        }
      }
    }
    for (const literal of file.literals) {
      const assignment = /lynx\.(?:held-?status|ok-?validity)\s*[=:]\s*(.+)$/i.exec(literal.value);
      if (assignment) consider(assignment[1]);
    }
  }
  for (const name of listFiles(TEST_RESOURCES).filter((file) => /\.(ya?ml|properties)$/.test(file))) {
    const entries = /\.properties$/.test(name) ? flattenProperties(readFileSync(name, "utf8")) : flattenYaml(readFileSync(name, "utf8"));
    for (const entry of entries) {
      if (SETTINGS.some((setting) => canonical(entry.key) === canonical(setting.key))) consider(entry.value);
    }
  }
  if (customLabels.size === 0) {
    fail("no test configures a label other than the defaults: add a test that imports an activity carrying non-default labels");
  }
}

// Which arguments of `new <properties class>(..)` are the two labels: by
// constructor parameter name, or by field order for a Lombok all-args
// constructor. null = unknown, every argument is considered.
function labelPositions(cls, argumentCount) {
  const ctor = cls.ctors.find((item) => item.params === argumentCount);
  let names = null;
  if (ctor) {
    names = ctor.paramNames;
  } else if (hasAnnotation(cls.annotations, ["AllArgsConstructor"])) {
    const fields = cls.fields.filter((field) => !field.static && !(field.final && field.init !== null));
    if (fields.length === argumentCount) names = fields.map((field) => field.name);
  }
  if (names === null) return null;
  const positions = [];
  names.forEach((name, index) => {
    if (SETTINGS.some((setting) => setting.property.toLowerCase() === name.toLowerCase())) positions.push(index);
  });
  return positions.length > 0 ? positions : null;
}

function finish() {
  if (failures.length > 0) {
    console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
    process.exit(1);
  }
  console.log("PASS (structural): lynx.heldStatus / lynx.okValidity are declared, validated, defaulted in application.yml and read by every consumer; tests updated");
  process.exit(0);
}

// ---------------------------------------------------------------- helpers
function hasAnnotation(annotations, names) {
  return annotations.some((item) => names.includes(item.name));
}

function simpleType(type) {
  if (!type) return "";
  const raw = type.replace(/<.*$/s, "").trim();
  return raw.slice(raw.lastIndexOf(".") + 1);
}

function capitalize(name) {
  return name.charAt(0).toUpperCase() + name.slice(1);
}

function canonical(key) {
  return key.toLowerCase().replace(/[-_]/g, "");
}

function unquote(value) {
  const trimmed = value.trim();
  const quoted = /^"(.*)"$/.exec(trimmed) ?? /^'(.*)'$/.exec(trimmed);
  return quoted ? quoted[1] : trimmed;
}

// Minimal YAML reader for Spring configuration files: nested maps with scalar
// values, comments, quoted scalars. Returns [{ key: "a.b.c", value }] in file
// order (a map key is listed with the value "").
function flattenYaml(text) {
  const entries = [];
  let stack = [];
  for (const raw of text.split(/\r?\n/)) {
    if (/^---/.test(raw)) {
      stack = [];
      continue;
    }
    if (/^\s*(#.*)?$/.test(raw)) continue;
    const match = /^(\s*)("[^"]*"|'[^']*'|[^\s:#"'][^:#]*?)\s*:(?:\s+(.*))?$/.exec(raw);
    if (!match) continue;
    const indent = match[1].replace(/\t/g, "    ").length;
    while (stack.length > 0 && stack[stack.length - 1].indent >= indent) stack.pop();
    const key = unquote(match[2]);
    let value = (match[3] ?? "").trim();
    if (value.startsWith("#")) value = "";
    if (!/^["']/.test(value)) value = value.replace(/\s+#.*$/, "").trim();
    else value = (/^("[^"]*"|'[^']*')/.exec(value)?.[1] ?? value);
    const path = [...stack.map((item) => item.key), key].join(".");
    entries.push({ key: path, value: unquote(value) });
    if (value === "") stack.push({ indent, key });
  }
  return entries;
}

function flattenProperties(text) {
  const entries = [];
  for (const raw of text.split(/\r?\n/)) {
    if (/^\s*([#!].*)?$/.test(raw)) continue;
    const match = /^\s*([^=:\s]+)\s*[=:]?\s*(.*)$/.exec(raw);
    if (match) entries.push({ key: match[1], value: match[2].trim() });
  }
  return entries;
}

function listFiles(root) {
  if (!existsSync(root)) return [];
  const result = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) result.push(...listFiles(path));
    else result.push(path);
  }
  return result;
}

function tail(text, lines = 40) {
  return (text ?? "").trim().split(/\r?\n/).slice(-lines).join("\n");
}
