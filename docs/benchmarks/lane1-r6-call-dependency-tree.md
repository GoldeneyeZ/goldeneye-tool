# Lane 1 R6 call dependency tree

Source:

- Run: `lane1-r6-direct-gcal-c-fc6d181-20260727/L0`
- Trace: `smoke/2026-07-27T15-55-19-569Z/goldeneye-code-agent-layer/smoke-spring-sensitive-value-redaction-level0-goldeneye-code-agent-layer/codex.jsonl`
- Result: `PASS`; 50 command executions, 4 patch operations, 5 agent messages

This is a dependency DAG, not only a chronological sequence. It shows whether a call consumed data or state produced by an earlier call.

## Legend

- Solid arrow, `A --> B`: exact response reuse. `B`'s requested qualified name occurs verbatim in `A`'s response.
- Dashed arrow, `A -.-> B`: inferred semantic reuse, query refinement, negative-result fallback, or failure-driven diagnosis.
- Thick arrow, `A ==> B`: mutation/verification control dependency.
- Red node: failed call.
- Gray node: empty/near-empty response.
- Nodes without incoming arrows: independent discovery roots; no prior-response dependency detected.

## Full dependency DAG

```mermaid
flowchart TD
  subgraph S0["Setup and validation flow"]
    E02["E02 gcal status"]
    E03["E03 search DataBinder<br/>limit 20"]
    E04["E04 get DataBinder.applyPropertyValues"]
    E05["E05 search binding/error classes"]
    E06["E06 get AbstractBindingResult.rejectValue"]
    E07["E07 search actual/raw/formatted field value"]
    E08["E08 get getActualFieldValue"]
    E09["E09 get processPropertyAccessException"]
    E10["E10 search addError --scope<br/>FAIL: unsupported option"]
    E11["E11 search addError --file"]
    E12["E12 get AbstractBindingResult.addError"]
    E13["E13 inspect AbstractPropertyBindingResult<br/>FAIL: incomplete QN; suggestions returned"]
  end

  subgraph S1["Annotation and metadata exploration"]
    E14["E14 search PropertyHandler / PropertyDescriptor / DirectFieldAccessor"]
    E15["E15 search getPropertyTypeDescriptor / getAnnotatedElement"]
    E16["E16 get getPropertyTypeDescriptor"]
    E17["E17 search TypeDescriptor / getAnnotations / hasAnnotation"]
    E18["E18 get TypeDescriptor.getAnnotations"]
    E19["E19 search getAnnotatedElement"]
    E20["E20 get TypeDescriptor.getAnnotatedElement"]
    E21["E21 search MergedAnnotations.from"]
    E22["E22 inspect MergedAnnotations.from"]
    E23["E23 get MergedAnnotations.from"]
    E24["E24 get MergedAnnotations.from#9"]
  end

  subgraph S2["Bean/direct-field type path"]
    E25["E25 search BeanPropertyHandler / toTypeDescriptor"]
    E26["E26 get BeanPropertyHandler constructor"]
    E27["E27 get BeanPropertyHandler.toTypeDescriptor"]
    E28["E28 search GenericTypeAwarePropertyDescriptor / getTypeDescriptor"]
    E29["E29 get GenericTypeAwarePropertyDescriptor.getTypeDescriptor"]
    E30["E30 search Property / getAnnotations<br/>EMPTY"]
    E31["E31 search Property.Property"]
    E32["E32 get Property.getAnnotations"]
    E33["E33 search resolveAnnotations"]
    E34["E34 get Property.resolveAnnotations"]
    E35["E35 get PropertyHandler.nested"]
    E36["E36 get BeanPropertyHandler.nested"]
    E37["E37 search TypeDescriptor.nested"]
    E38["E38 get TypeDescriptor.nested"]
    E39["E39 search DataBinderTests / initDirectFieldAccess"]
    E40["E40 search annotation FIELD+METHOD target<br/>EMPTY"]
  end

  subgraph S3["Implementation"]
    E42["E42 get AbstractPropertyBindingResult<br/>exact QN recovered from E13 suggestions"]
    P1["E43 patch<br/>AbstractBindingResult<br/>DefaultBindingErrorProcessor<br/>Sensitive"]
    E44["E44 search PropertyEditor / TypeDescriptor<br/>EMPTY"]
    P2["E45 patch<br/>AbstractPropertyBindingResult"]
    E46["E46 git diff --check + focused diff"]
    P3["E47 patch<br/>AbstractPropertyBindingResult<br/>DataBinderSensitiveValueTests"]
  end

  subgraph S4["Verification and repair"]
    T1["E49 focused Gradle test<br/>FAIL: 3/4"]
    E50["E50 read failed-test XML"]
    E51["E51 search MergedAnnotations.from(...).isPresent<br/>EMPTY"]
    E52["E52 search hasAnnotation"]
    E53["E53 get AnnotatedElementUtils.hasAnnotation"]
    E54["E54 get Property.getField<br/>semantic target; no exact prior-QN match"]
    P4["E55 corrective patch<br/>AbstractPropertyBindingResult"]
    T2["E57 focused Gradle test<br/>PASS"]
    E58["E58 git diff --check + status + stat<br/>PASS"]
  end

  E03 -->|"exact QN"| E04
  E05 -->|"exact QN"| E06
  E05 -->|"exact QN"| E09
  E06 -. "term: getActualFieldValue" .-> E07
  E07 -->|"exact QN"| E08
  E07 -->|"exact QN"| E13
  E09 -. "term: addError" .-> E10
  E10 -. "CLI error corrected: --scope → --file" .-> E11
  E11 -->|"exact QN"| E12

  E05 -. "term: DirectFieldAccessor" .-> E14
  E15 -->|"exact QN"| E16
  E16 -. "term: TypeDescriptor" .-> E17
  E17 -->|"exact QN"| E18
  E18 -. "term: getAnnotatedElement" .-> E19
  E19 -->|"exact QN"| E20
  E21 -->|"exact QN"| E22
  E22 -->|"exact QN"| E23
  E21 -->|"exact QN"| E24

  E16 -. "term: toTypeDescriptor" .-> E25
  E25 -->|"exact QN"| E26
  E25 -->|"exact QN"| E27
  E27 -. "term: getTypeDescriptor" .-> E28
  E28 -->|"exact QN"| E29
  E29 -. "term: Property" .-> E30
  E30 -. "empty result → reformulation" .-> E31
  E31 -->|"exact QN"| E32
  E32 -. "term: resolveAnnotations" .-> E33
  E33 -->|"exact QN"| E34
  E14 -->|"exact QN; long-range reuse"| E35
  E25 -->|"exact QN; long-range reuse"| E36
  E36 -. "terms: TypeDescriptor + nested" .-> E37
  E37 -->|"exact QN"| E38

  E13 -->|"exact suggestion reused after 28 events"| E42
  E06 ==> P1
  E09 ==> P1
  E40 -. "annotation-contract lookup; empty result" .-> P1
  E42 ==> P1
  P1 ==> P2
  E42 ==> P2
  E44 -. "negative lookup" .-> P2
  P1 ==> E46
  P2 ==> E46
  E39 -. "test-surface discovery" .-> P3
  E46 ==> P3

  P3 ==> T1
  T1 -. "failure details" .-> E50
  T1 -. "3 failing behaviors" .-> E51
  E46 -. "terms: MergedAnnotations + isPresent" .-> E51
  E51 -. "empty result → fallback" .-> E52
  E17 -. "term: hasAnnotation; long-range reuse" .-> E52
  E52 -->|"exact QN"| E53
  E34 -. "Property source → inferred getField target" .-> E54
  T1 ==> P4
  E53 ==> P4
  E54 ==> P4
  P4 ==> T2
  T2 ==> E58

  classDef fail fill:#fee2e2,stroke:#dc2626,color:#7f1d1d
  classDef empty fill:#f3f4f6,stroke:#6b7280,color:#374151
  classDef patch fill:#dcfce7,stroke:#16a34a,color:#14532d
  classDef verify fill:#dbeafe,stroke:#2563eb,color:#1e3a8a
  class E10,E13,T1 fail
  class E30,E40,E44,E51 empty
  class P1,P2,P3,P4 patch
  class T2,E58 verify
```

## Response-reuse ledger

| Consumer class | Calls | Prior-response use |
|---|---:|---|
| `gcal get` / `gcal inspect` | 23 | 22 exact QN matches; E54 semantic |
| `gcal search` | 21 | 15 derived/refined/retry searches; 6 independent roots |
| Patch operations | 4 | All depend on retrieved source, diff, or test failure |
| Verification/diagnostics | 5 | Patches → tests; failed test → XML/search repair; passing test → final diff |

Independent search roots:

- E03: `DataBinder`
- E05: binding/error classes
- E15: `getPropertyTypeDescriptor|getAnnotatedElement`
- E21: `MergedAnnotations.from`
- E39: `DataBinderTests|initDirectFieldAccess`
- E40: annotation `FIELD` + `METHOD` target

High-value reuse:

- E13 failed, but its suggestions were retained and used exactly by E42 after 28 intervening events.
- E14 fed E35 after 21 events.
- E17 fed E52 after 35 events.
- E25 fed E36 after 11 events.

Correction chains:

- E10 invalid `--scope` → E11 corrected `--file` → E12 exact `get`.
- E30 empty → E31 reformulated search → E32 exact `get`.
- T1 failed 3/4 → E50/E51 diagnosis → E52/E53/E54 repair evidence → P4 → T2 pass.

## Main finding

Prior responses were heavily reused: 22/23 source retrievals have exact machine-verifiable ancestry. Main inefficiency is excessive fan-out, not discarded context: six root searches expanded into 21 searches and 23 retrieval/inspection calls before convergence.
