#!/usr/bin/env node
// Grader for the `vivwallet-feature-dirty` task (payments summary endpoint).
//
//   node vivwallet-feature-dirty.mjs <worktree> [<source repo>]
//
// The project's Maven dependencies (Spring Boot 2.1.3) cannot be resolved
// offline, so this grader is STRUCTURAL: it reads the Java sources, strips
// comments, and checks that
//   - the overlay's work in progress is still there (renamed Payment.viv,
//     VivAmounts helper, controller/service additions) and no pre-overlay
//     name is used anywhere;
//   - GET /api/v1/payments/summary exists with optional ISO `from`/`to`
//     parameters, administrator restriction, a 400 for an inverted period and
//     a delegation to PaymentService.summarize(LocalDate, LocalDate);
//   - PaymentsSummaryDto exposes count / totalViv / totalAmount;
//   - the euro total goes through VivAmounts.toEuros and the rate is not
//     duplicated;
//   - a service test calls summarize;
//   - every Java file under ViV-Wallet-api/src still parses at Java 11
//     (JDK compiler in parse-only mode, no classpath needed).
//
// Exit 0 = pass. Failures are printed to stderr.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const worktree = resolve(process.argv[2] ?? ".");
const here = dirname(fileURLToPath(import.meta.url));
const syntaxChecker = join(here, "fixtures", "vivwallet-feature-dirty.JavaSyntaxCheck.java");

const SRC = join(worktree, "ViV-Wallet-api", "src");
const MAIN = join(SRC, "main", "java", "com", "invivoo", "vivwallet", "api");
const TEST = join(SRC, "test", "java", "com", "invivoo", "vivwallet", "api");
const PATHS = {
  payment: join(MAIN, "domain", "payment", "Payment.java"),
  helper: join(MAIN, "domain", "payment", "VivAmounts.java"),
  repository: join(MAIN, "domain", "payment", "PaymentRepository.java"),
  service: join(MAIN, "domain", "payment", "PaymentService.java"),
  paymentDto: join(MAIN, "interfaces", "payments", "PaymentDto.java"),
  summaryDto: join(MAIN, "interfaces", "payments", "PaymentsSummaryDto.java"),
  controller: join(MAIN, "interfaces", "payments", "PaymentsController.java"),
  userService: join(MAIN, "domain", "user", "UserService.java"),
  provider: join(MAIN, "application", "provider", "payments", "PaymentsProviderService.java"),
  serviceTest: join(TEST, "domain", "payment", "PaymentServiceTest.java"),
};

const failures = [];
const fail = (message) => failures.push(message);

if (!existsSync(MAIN)) {
  fail(`not a ViV-Wallet worktree: ${worktree}`);
  finish();
}

// ---------------------------------------------------------------- sources
// code[k]   : source without comments (string literals kept)
// masked[k] : same length as code[k], string/char literal contents blanked
const code = {};
const masked = {};
for (const [key, path] of Object.entries(PATHS)) {
  if (!existsSync(path)) continue;
  const stripped = stripComments(readFileSync(path, "utf8"));
  code[key] = stripped.code;
  masked[key] = stripped.masked;
}
const has = (key, pattern) => key in code && pattern.test(code[key]);

checkOverlayIntact();
checkNoStaleNames();
const summary = checkEndpoint();
checkService();
checkSummaryDto();
checkConversion(summary);
checkTests();
checkSyntax();
finish();

// ---------------------------------------------------------------- checks
function checkOverlayIntact() {
  if (!("helper" in code)) {
    fail("work in progress lost: domain/payment/VivAmounts.java is missing");
  } else {
    if (!/static\s+BigDecimal\s+toEuros\s*\(\s*int\s+\w+\s*\)/.test(code.helper)) {
      fail("work in progress changed: VivAmounts.toEuros(int) is missing");
    }
    if (!/EUROS_PER_VIV\s*=\s*5\s*;/.test(code.helper)) {
      fail("work in progress changed: VivAmounts.EUROS_PER_VIV = 5 is missing");
    }
  }
  if (!has("payment", /private\s+int\s+viv\s*;/) || !has("payment", /@Column\s*\(\s*name\s*=\s*"viv_amount"\s*\)/)) {
    fail('work in progress lost: Payment must keep `@Column(name = "viv_amount") private int viv;`');
  }
  if (has("payment", /vivAmount/)) fail("Payment still (or again) declares vivAmount");
  if (!has("paymentDto", /VivAmounts\s*\.\s*toEuros\s*\(\s*payment\s*\.\s*getViv\s*\(\s*\)\s*\)/)) {
    fail("work in progress lost: PaymentDto must keep converting with VivAmounts.toEuros(payment.getViv())");
  }
  if (!has("controller", /String\s+COMPANY_ADMINISTRATOR_ONLY\s*=\s*"hasAnyAuthority\('COMPANY_ADMINISTRATOR'\)"\s*;/)) {
    fail("work in progress lost: PaymentsController.COMPANY_ADMINISTRATOR_ONLY is missing");
  }
  if (!has("controller", /String\s+API_V_1_PAYMENTS\s*=\s*"\/api\/v1\/payments"\s*;/) ||
      !has("controller", /@RequestMapping\s*\(\s*PaymentsController\s*\.\s*API_V_1_PAYMENTS\s*\)/)) {
    fail("PaymentsController must stay mapped on /api/v1/payments through API_V_1_PAYMENTS");
  }
  for (const [name, pattern] of [
    ["getPayments", /ResponseEntity\s*<\s*List\s*<\s*PaymentDto\s*>\s*>\s+getPayments\s*\(\s*\)/],
    ["postPayment", /@PostMapping[\s\S]*?\bpostPayment\s*\(/],
    ["deletePayment", /@DeleteMapping\s*\(\s*"\/\{id\}"\s*\)[\s\S]*?\bdeletePayment\s*\(/],
  ]) {
    if (!has("controller", pattern)) fail(`existing endpoint ${name} was removed or changed`);
  }
  if (!has("service", /public\s+boolean\s+existsById\s*\(\s*Long\s+\w+\s*\)/)) {
    fail("work in progress lost: PaymentService.existsById(Long) is missing");
  }
  for (const method of ["findById", "getAll", "findAllByReceiver", "findByDateAndReceiver", "save", "saveAll", "deleteAll", "delete"]) {
    if (!has("service", new RegExp(`public\\s+[\\w<>\\[\\], ]+\\s+${method}\\s*\\(`))) {
      fail(`existing PaymentService.${method} was removed`);
    }
  }
  if (!has("userService", /Payment\s*::\s*getViv\b/)) {
    fail("work in progress lost: UserService must keep summing Payment::getViv");
  }
  if (!has("provider", /\.\s*viv\s*\(\s*vivAmount\s*\)/)) {
    fail("work in progress lost: PaymentsProviderService must keep building payments with .viv(vivAmount)");
  }
}

function checkNoStaleNames() {
  for (const file of javaFiles(SRC)) {
    const name = relative(worktree, file).replaceAll("\\", "/");
    const stripped = stripComments(readFileSync(file, "utf8"));
    const text = stripped.masked;
    if (/FACTOR_VIV_TO_AMOUNT/.test(text)) fail(`${name}: uses PaymentDto.FACTOR_VIV_TO_AMOUNT, which no longer exists`);
    if (/Payment\s*::\s*getVivAmount/.test(text)) fail(`${name}: uses Payment::getVivAmount, the property is now Payment.viv`);
    for (const match of text.matchAll(/\b([A-Za-z_]\w*)\s*\.\s*getVivAmount\s*\(/g)) {
      const receiver = match[1];
      if (/payment/i.test(receiver) && !/request/i.test(receiver)) {
        fail(`${name}: ${receiver}.getVivAmount() - the Payment property is now viv`);
      }
    }
    const builderStart = /(?<![\w.])Payment\s*\.\s*builder\s*\(\s*\)|\bPayment\s*::\s*(?:builder|toBuilder)\b|\b\w*[pP]ayment\w*\s*\.\s*toBuilder\s*\(\s*\)/g;
    for (const match of text.matchAll(builderStart)) {
      const rest = text.slice(match.index + match[0].length);
      const end = rest.search(/\.\s*build\s*\(\s*\)|;/);
      const chain = end === -1 ? rest : rest.slice(0, end);
      if (/\.\s*vivAmount\s*\(/.test(chain)) {
        fail(`${name}: a Payment builder still calls .vivAmount(...), the property is now viv`);
      }
    }
  }
  for (const key of ["service", "summaryDto"]) {
    if (has(key, /getVivAmount|\bvivAmount\b/)) fail(`${key}: refers to vivAmount, the Payment property is now viv`);
  }
}

function checkEndpoint() {
  if (!("controller" in code)) {
    fail("PaymentsController.java is missing");
    return null;
  }
  const text = code.controller;
  const mask = masked.controller;
  let found = null;
  for (const match of mask.matchAll(/@(GetMapping|RequestMapping)\b/g)) {
    let cursor = match.index + match[0].length;
    while (/\s/.test(mask[cursor] ?? "")) cursor += 1;
    let args = "";
    let afterAnnotation = match.index + match[0].length;
    if (mask[cursor] === "(") {
      const close = matching(mask, cursor, "(", ")");
      if (close === -1) continue;
      args = text.slice(cursor + 1, close);
      afterAnnotation = close + 1;
    }
    if (match[1] === "RequestMapping" && !/RequestMethod\s*\.\s*GET\b/.test(args)) continue;
    const path = mappingPath(args, text);
    if (path === null || path.replace(/^\/+|\/+$/g, "") !== "summary") continue;
    found = { annotationIndex: match.index, afterAnnotation };
    break;
  }
  if (!found) {
    fail('PaymentsController has no GET mapping for "/summary" (expected GET /api/v1/payments/summary)');
    return null;
  }

  // method block: from the end of the previous member to the closing brace
  let blockStart = found.annotationIndex;
  while (blockStart > 0 && !/[;{}]/.test(mask[blockStart - 1])) blockStart -= 1;
  let bodyStart = -1;
  let depth = 0;
  for (let index = found.afterAnnotation; index < mask.length; index += 1) {
    const char = mask[index];
    if (char === "(") depth += 1;
    else if (char === ")") depth -= 1;
    else if (char === "{" && depth === 0) {
      bodyStart = index;
      break;
    } else if (char === ";" && depth === 0) break;
  }
  if (bodyStart === -1) {
    fail("could not find the body of the /summary handler method");
    return null;
  }
  const bodyEnd = matching(mask, bodyStart, "{", "}");
  const block = text.slice(blockStart, bodyEnd === -1 ? text.length : bodyEnd + 1);
  const header = text.slice(blockStart, bodyStart);
  const headerMask = mask.slice(blockStart, bodyStart);

  if (!/@PreAuthorize\s*\(\s*(?:PaymentsController\s*\.\s*)?COMPANY_ADMINISTRATOR_ONLY\s*\)/.test(block) &&
      !/@PreAuthorize\s*\(\s*"[^"]*COMPANY_ADMINISTRATOR[^"]*"\s*\)/.test(block)) {
    fail("the /summary endpoint is not restricted to company administrators (@PreAuthorize)");
  }

  // parameter list = last balanced (...) group of the header
  const closeParen = headerMask.lastIndexOf(")");
  let openParen = -1;
  for (let index = closeParen, level = 0; index >= 0; index -= 1) {
    if (headerMask[index] === ")") level += 1;
    else if (headerMask[index] === "(") {
      level -= 1;
      if (level === 0) {
        openParen = index;
        break;
      }
    }
  }
  const params = openParen === -1 ? [] : splitTopLevel(header.slice(openParen + 1, closeParen), headerMask.slice(openParen + 1, closeParen));
  const stringParams = [];
  for (const wanted of ["from", "to"]) {
    const param = params.find((candidate) => requestParamName(candidate) === wanted);
    if (!param) {
      fail(`the /summary endpoint has no @RequestParam named "${wanted}"`);
      continue;
    }
    if (!/required\s*=\s*false/.test(param) && !/\bOptional\s*</.test(param)) {
      fail(`request parameter "${wanted}" must be optional (required = false)`);
    }
    if (/\bLocalDate\b/.test(param)) {
      const iso = /@DateTimeFormat\s*\([^)]*(?:\biso\s*=\s*(?:[\w.]+\s*\.\s*)?DATE\b|"yyyy-MM-dd")/.test(param);
      if (!iso && !globalIsoDateFormat()) {
        fail(`request parameter "${wanted}" is a LocalDate without an ISO date format (@DateTimeFormat(iso = ISO.DATE)); Spring would reject yyyy-MM-dd`);
      }
    } else if (/\bString\b/.test(param)) {
      stringParams.push(wanted);
    } else {
      fail(`request parameter "${wanted}" must be a date (LocalDate) or a String parsed as an ISO date`);
    }
  }
  if (stringParams.length > 0 && !/LocalDate\s*(?:\.\s*parse\s*\(|::\s*parse\b)/.test(`${text}\n${code.service ?? ""}`)) {
    fail(`request parameters ${stringParams.join(", ")} are Strings but are never parsed with LocalDate.parse`);
  }

  if (!/\bpaymentService\s*\.\s*summarize\s*\(/.test(block)) {
    fail("the /summary handler does not delegate to paymentService.summarize(...)");
  }
  const service = code.service ?? "";
  const badRequest = /badRequest\s*\(|BAD_REQUEST|status\s*\(\s*400\s*\)/;
  if (!badRequest.test(block) && !/BAD_REQUEST/.test(service) &&
      !(/@ExceptionHandler/.test(text) && badRequest.test(text))) {
    fail("no 400 Bad Request path for a period whose `from` is after `to`");
  }
  if (!/\b(isAfter|isBefore|compareTo)\s*\(/.test(`${block}\n${service}`)) {
    fail("`from` and `to` are never compared (needed to reject an inverted period and to filter)");
  }
  return { block };
}

function checkService() {
  if (!("service" in code)) {
    fail("PaymentService.java is missing");
    return;
  }
  const match = /public\s+(?:final\s+)?PaymentsSummaryDto\s+summarize\s*\(([^)]*)\)/.exec(masked.service);
  if (!match) {
    fail("PaymentService has no `public PaymentsSummaryDto summarize(LocalDate from, LocalDate to)`");
    return;
  }
  const params = match[1].split(",").map((param) => param.trim()).filter(Boolean);
  if (params.length !== 2 || !params.every((param) => /\bLocalDate\b/.test(param))) {
    fail("PaymentService.summarize must take exactly two LocalDate parameters (from, to)");
  }
  const repository = code.repository ?? "";
  const summed = /\bgetViv\s*\(|::\s*getViv\b/.test(`${code.service}\n${code.summaryDto ?? ""}`) ||
    /@Query\b[\s\S]{0,600}?\.viv\b/.test(repository);
  if (!summed) fail("the viv total is not computed from Payment.viv (getViv)");
  if (!/\b(isAfter|isBefore|compareTo)\s*\(|Date(Between|After|Before|GreaterThan|LessThan)|@Query|Specification/.test(`${code.service}\n${repository}`)) {
    fail("payments are not filtered on their date in PaymentService/PaymentRepository");
  }
}

function checkSummaryDto() {
  if (!("summaryDto" in code)) {
    fail("interfaces/payments/PaymentsSummaryDto.java is missing");
    return;
  }
  const text = code.summaryDto;
  if (!/package\s+com\.invivoo\.vivwallet\.api\.interfaces\.payments\s*;/.test(text) ||
      !/\b(class|record)\s+PaymentsSummaryDto\b/.test(text)) {
    fail("PaymentsSummaryDto must be a class in package com.invivoo.vivwallet.api.interfaces.payments");
  }
  const field = (type, name) => new RegExp(`\\b(?:${type})\\s+${name}\\s*[;=,)]`).test(text);
  if (!field("long|int|Long|Integer", "count")) fail("PaymentsSummaryDto needs an integer field `count`");
  if (!field("long|int|Long|Integer", "totalViv")) fail("PaymentsSummaryDto needs an integer field `totalViv`");
  if (!field("BigDecimal", "totalAmount")) fail("PaymentsSummaryDto needs a BigDecimal field `totalAmount`");
  const lombok = /@(Data|Getter|Value)\b/.test(text);
  const explicit = ["getCount", "getTotalViv", "getTotalAmount"].every((getter) => new RegExp(`\\b${getter}\\s*\\(`).test(text));
  if (!lombok && !explicit && !/\brecord\s+PaymentsSummaryDto\b/.test(text)) {
    fail("PaymentsSummaryDto exposes no getters (use the Lombok conventions of PaymentDto)");
  }
}

function checkConversion(summaryEndpoint) {
  const scopes = [code.service ?? "", code.summaryDto ?? "", summaryEndpoint?.block ?? ""];
  const joined = scopes.join("\n");
  if (!/VivAmounts/.test(joined) || !/\btoEuros\s*\(/.test(joined)) {
    fail("totalAmount is not computed with the existing VivAmounts.toEuros helper");
  }
  const duplicated = [
    [/EUROS_PER_VIV/, "uses the raw rate EUROS_PER_VIV instead of VivAmounts.toEuros"],
    [/\*\s*5\b(?!\s*\.\d)|\b5\s*\*/, "multiplies by a hard-coded 5"],
    [/valueOf\s*\(\s*5L?\s*\)|new\s+BigDecimal\s*\(\s*"?5"?\s*\)/, "builds a hard-coded BigDecimal 5"],
    [/=\s*5\s*;/, "declares a hard-coded rate of 5"],
  ];
  for (const [pattern, message] of duplicated) {
    if (pattern.test(`${joined}\n${code.repository ?? ""}`)) fail(`the viv-to-euro conversion is duplicated: ${message}`);
  }
}

function checkTests() {
  if (!("serviceTest" in code) || !/\.\s*summarize\s*\(/.test(code.serviceTest)) {
    fail("PaymentServiceTest has no test calling summarize(...)");
  }
}

function checkSyntax() {
  if (!existsSync(syntaxChecker)) {
    fail(`syntax checker fixture missing: ${syntaxChecker}`);
    return;
  }
  const run = spawnSync("java", [syntaxChecker, "11", SRC], {
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
    timeout: 150_000,
    windowsHide: true,
  });
  if (run.error) {
    fail(`could not run the Java syntax check (java 11+ required on PATH): ${run.error.message}`);
  } else if (run.status === 1) {
    fail(`Java sources do not parse at Java 11:\n${tail(run.stdout)}`);
  } else if (run.status !== 0) {
    fail(`Java syntax check failed to run (exit ${run.status}):\n${tail(`${run.stdout}\n${run.stderr}`)}`);
  }
}

function finish() {
  if (failures.length > 0) {
    console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
    process.exit(1);
  }
  console.log("PASS (structural): payments summary endpoint, service method, DTO, helper reuse and overlay state verified");
  process.exit(0);
}

// ---------------------------------------------------------------- helpers
function globalIsoDateFormat() {
  const yml = join(SRC, "main", "resources", "application.yml");
  if (existsSync(yml) && /date-format\s*:|format\s*:\s*\r?\n\s+date\s*:/.test(readFileSync(yml, "utf8"))) return true;
  return javaFiles(join(SRC, "main")).some((file) =>
    /DateTimeFormatterRegistrar|Converter\s*<\s*String\s*,\s*LocalDate\s*>|Formatter\s*<\s*LocalDate\s*>/.test(
      stripComments(readFileSync(file, "utf8")).code,
    ),
  );
}

function requestParamName(param) {
  if (!/@RequestParam\b/.test(param)) return null;
  const explicit = /@RequestParam\s*\(\s*(?:(?:value|name)\s*=\s*)?"([^"]*)"/.exec(param) ??
    /@RequestParam\s*\([^)]*\b(?:value|name)\s*=\s*"([^"]*)"/.exec(param);
  if (explicit) return explicit[1];
  const identifiers = param.replace(/"[^"]*"/g, " ").match(/[A-Za-z_]\w*/g) ?? [];
  return identifiers.length > 0 ? identifiers[identifiers.length - 1] : null;
}

function mappingPath(args, fileText) {
  const trimmed = args.trim();
  if (trimmed === "") return "";
  const named = /\b(?:value|path)\s*=\s*([^,]+)/.exec(trimmed);
  let expression = named ? named[1] : /=/.test(trimmed.replace(/"[^"]*"/g, "")) ? null : trimmed;
  if (expression === null) return "";
  expression = expression.trim().replace(/^\{\s*|\s*\}$/g, "").trim();
  const literal = /^"([^"]*)"$/.exec(expression);
  if (literal) return literal[1];
  const constant = /^(?:\w+\s*\.\s*)*(\w+)$/.exec(expression);
  if (constant) {
    const definition = new RegExp(`String\\s+${constant[1]}\\s*=\\s*"([^"]*)"\\s*;`).exec(fileText);
    if (definition) return definition[1];
  }
  return null;
}

function splitTopLevel(text, mask) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < mask.length; index += 1) {
    const char = mask[index];
    if (char === "(" || char === "<" || char === "{" || char === "[") depth += 1;
    else if (char === ")" || char === ">" || char === "}" || char === "]") depth -= 1;
    else if (char === "," && depth === 0) {
      parts.push(text.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

function matching(mask, openIndex, open, close) {
  let depth = 0;
  for (let index = openIndex; index < mask.length; index += 1) {
    if (mask[index] === open) depth += 1;
    else if (mask[index] === close) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

// Removes // and /* */ comments. `code` keeps string literals, `masked` blanks
// their contents; both keep the original length and line structure.
function stripComments(source) {
  let code = "";
  let masked = "";
  let index = 0;
  while (index < source.length) {
    const char = source[index];
    const next = source[index + 1];
    if (char === "/" && next === "/") {
      while (index < source.length && source[index] !== "\n") {
        code += " ";
        masked += " ";
        index += 1;
      }
    } else if (char === "/" && next === "*") {
      const end = source.indexOf("*/", index + 2);
      const stop = end === -1 ? source.length : end + 2;
      for (; index < stop; index += 1) {
        const keep = source[index] === "\n" || source[index] === "\r" ? source[index] : " ";
        code += keep;
        masked += keep;
      }
    } else if (char === '"' || char === "'") {
      code += char;
      masked += char;
      index += 1;
      while (index < source.length && source[index] !== char && source[index] !== "\n") {
        if (source[index] === "\\" && index + 1 < source.length) {
          code += source[index] + source[index + 1];
          masked += "  ";
          index += 2;
        } else {
          code += source[index];
          masked += " ";
          index += 1;
        }
      }
      if (index < source.length) {
        code += source[index];
        masked += source[index];
        index += 1;
      }
    } else {
      code += char;
      masked += char;
      index += 1;
    }
  }
  return { code, masked };
}

function javaFiles(root) {
  if (!existsSync(root)) return [];
  const result = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) result.push(...javaFiles(path));
    else if (entry.endsWith(".java")) result.push(path);
  }
  return result;
}

function tail(text, lines = 40) {
  return (text ?? "").trim().split(/\r?\n/).slice(-lines).join("\n");
}
