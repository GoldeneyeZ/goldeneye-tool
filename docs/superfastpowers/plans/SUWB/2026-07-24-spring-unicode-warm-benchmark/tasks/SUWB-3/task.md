# Task 3: Add Spring task, held-out grader, and benchmark configuration

<TASK-ID>SUWB-3</TASK-ID>

**Files:**

- Create: `tools/agent-bench/tasks/spring-stringutils-unicode-truncate.md`
- Create: `tools/agent-bench/graders/spring-stringutils-unicode-truncate.ps1`
- Create: `tools/agent-bench/configs/spring-stringutils-unicode-truncate.json`
- Create: `tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1`

**Step 1: Write the task prompt**

Use this exact behavioral contract:

```markdown
Update Spring Framework `spring-core` method
`org.springframework.util.StringUtils.truncate(CharSequence, int)`.

When truncation is required, the UTF-16 prefix must never end between the high
and low surrogate of one valid surrogate pair. If `threshold` falls between
that pair, shorten the prefix by one UTF-16 code unit before appending the
existing truncation suffix.

Preserve:
- existing positive-threshold precondition and message;
- existing suffix;
- existing behavior when `length() <= threshold`;
- existing UTF-16 code-unit threshold semantics in all other cases;
- `CharSequence` support.

Add focused coverage to
`spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`.

Run:
`.\gradlew.bat :spring-core:test --tests org.springframework.util.StringUtilsTests --build-cache`

Do not run `clean`. Do not change public API or unrelated files.
```

**Step 2: Write failing grader self-tests**

The grader self-test must create controlled patches in a disposable Spring
worktree and prove:

- old implementation fails held-out surrogate-split test;
- boundary-safe implementation passes;
- implementation that changes suffix fails;
- implementation that changes threshold precondition fails;
- solution without changes to `StringUtilsTests.java` fails protocol;
- grader restores/removes its held-out file after every outcome.

Run:

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
pwsh -NoProfile -File tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1
```

Expected: FAIL until grader exists.

**Step 3: Implement held-out grader**

The grader creates:

`spring-core/src/test/java/org/springframework/util/AgentBenchStringUtilsUnicodeTests.java`

with:

```java
package org.springframework.util;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatIllegalArgumentException;

class AgentBenchStringUtilsUnicodeTests {

	@Test
	void truncateDoesNotSplitSurrogatePair() {
		assertThat(StringUtils.truncate("abc😀rest", 4))
				.isEqualTo("abc (truncated)...");
	}

	@Test
	void truncateKeepsCompletePairAtThreshold() {
		assertThat(StringUtils.truncate("abc😀rest", 5))
				.isEqualTo("abc😀 (truncated)...");
	}

	@Test
	void truncateSupportsOtherCharSequenceImplementations() {
		assertThat(StringUtils.truncate(new StringBuilder("abc😀rest"), 4))
				.isEqualTo("abc (truncated)...");
	}

	@Test
	void truncatePreservesUntruncatedAndPreconditionBehavior() {
		assertThat(StringUtils.truncate("abc😀", 5)).isEqualTo("abc😀");
		assertThatIllegalArgumentException()
				.isThrownBy(() -> StringUtils.truncate("abc", 0))
				.withMessage("Truncation threshold must be a positive number: 0");
	}
}
```

Before testing, require:

- only allowed Spring paths are dirty:
  - `spring-core/src/main/java/org/springframework/util/StringUtils.java`
  - `spring-core/src/test/java/org/springframework/util/StringUtilsTests.java`
- production file changed;
- repository test file changed;
- held-out filename did not already exist.

Run:

```powershell
.\gradlew.bat :spring-core:test --tests org.springframework.util.AgentBenchStringUtilsUnicodeTests --build-cache
```

Always remove held-out source in `finally`. Then run:

```powershell
git diff --check
git status --short
```

Fail on unexpected paths, test failure, protocol failure, or cleanup failure.
Capture stdout, stderr, exit status, start/end timestamps, and duration.

**Step 4: Add frozen configuration**

Create configuration containing:

```json
{
  "name": "spring-stringutils-unicode-truncate",
  "repo": "D:\\Dev\\IdeaProjects\\spring-framework",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "model": "gpt-5.6-terra",
  "reasoning": "high",
  "repetitions": 3,
  "cache_modes": ["warm"],
  "java_home": "C:\\Users\\Zacha\\.jdks\\openjdk-17.0.2",
  "gradle_user_home": "D:\\Dev\\Caches\\gradle-spring-framework-6.2",
  "ready_snapshot": {
    "root": "../../../target/agent-bench/snapshots/spring-stringutils",
    "worktree": "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    "live_cache": "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
    "allowed_worktree_root": "D:\\Dev\\IdeaProjects\\.gab",
    "allowed_cache_root": "D:\\Dev\\IdeaProjects\\.gab-cache",
    "allowed_snapshot_root": "D:\\Dev\\IdeaProjects\\goldeneye-tool\\target\\agent-bench\\snapshots"
  },
  "task": {
    "id": "spring-stringutils-unicode-truncate",
    "prompt": "../tasks/spring-stringutils-unicode-truncate.md",
    "grader": "../graders/spring-stringutils-unicode-truncate.ps1",
    "extensions": [".java"]
  },
  "engines": [
    {
      "id": "goldeneye-code-agent-layer",
      "kind": "gcal",
      "command": "C:\\nvm4w\\nodejs\\node.exe",
      "args": [
        "D:\\Dev\\IdeaProjects\\goldeneye-tool\\agent\\dist\\main.js"
      ],
      "backend_command": "../../../target/release/goldeneye.exe",
      "cache_modes": ["warm"]
    },
    {
      "id": "vanilla",
      "kind": "vanilla",
      "cache_modes": ["none"]
    }
  ]
}
```

Adjust property names only to match existing validated harness schema. Do not
change fixed values or semantics.

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config tools/agent-bench/configs/spring-stringutils-unicode-truncate.json --dry-run
```

Expected: tests PASS; dry run prints one task, candidate three repetitions,
vanilla selectable as one override run, resolved absolute paths, and no
filesystem mutations.

**Step 5: Prime Gradle dependency/build cache outside scoring**

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
Set-Location 'D:\Dev\IdeaProjects\spring-framework'
.\gradlew.bat :spring-core:test --tests org.springframework.util.StringUtilsTests --build-cache
```

Expected: `BUILD SUCCESSFUL`. Never run `clean`.

**Step 6: Re-run grader self-test**

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
pwsh -NoProfile -File tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1
```

Expected: all negative and positive grader cases PASS; source repository returns
clean at pinned commit.

**Step 7: Commit only Task 3 files**

```powershell
git add -- tools/agent-bench/tasks/spring-stringutils-unicode-truncate.md tools/agent-bench/graders/spring-stringutils-unicode-truncate.ps1 tools/agent-bench/graders/spring-stringutils-unicode-truncate.test.ps1 tools/agent-bench/configs/spring-stringutils-unicode-truncate.json
git diff --cached --check
git commit -m "bench: add Spring Unicode truncate task"
```

Expected: commit contains only four Task 3 files.
