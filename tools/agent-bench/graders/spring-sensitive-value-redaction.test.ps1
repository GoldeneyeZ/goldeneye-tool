[CmdletBinding()]
param(
	[string]$Grader = (Join-Path $PSScriptRoot "spring-sensitive-value-redaction.ps1"),
	[ValidateSet(1, 2)]
	[int]$Level = 2
)

$ErrorActionPreference = "Stop"
$PowerShell = (Get-Command pwsh -ErrorAction Stop).Source
$FixtureRoot = Join-Path $PSScriptRoot "fixtures/spring-sensitive-value-redaction"
$FixtureTargets = @(
	"spring-core/src/test/java/org/springframework/core/annotation/SensitiveAnnotationAgentBenchTests.java",
	"spring-context/src/test/java/org/springframework/validation/SensitiveDataBinderAgentBenchTests.java",
	"spring-context/src/test/java/org/springframework/validation/beanvalidation/SensitiveMethodValidationAgentBenchTests.java",
	"spring-web/src/test/java/org/springframework/web/bind/support/SensitiveWebBindingInitializerAgentBenchTests.java",
	"spring-webmvc/src/test/java/org/springframework/web/servlet/mvc/method/annotation/SensitiveMvcAgentBenchTests.java",
	"spring-webflux/src/test/java/org/springframework/web/reactive/result/method/annotation/SensitiveWebFluxAgentBenchTests.java"
)
$ValidCandidatePaths = @(
	"spring-core/src/main/java/org/springframework/core/annotation/Sensitive.java",
	"spring-beans/src/main/java/org/springframework/beans/SensitiveBeanSupport.java",
	"spring-context/src/main/java/org/springframework/validation/SensitiveSupport.java",
	"spring-web/src/main/java/org/springframework/web/bind/SensitiveWebSupport.java",
	"spring-webmvc/src/main/java/org/springframework/web/servlet/SensitiveMvcSupport.java",
	"spring-webflux/src/main/java/org/springframework/web/reactive/SensitiveWebFluxSupport.java",
	"spring-context/src/test/java/org/springframework/validation/SensitiveSupportTests.java",
	"spring-webmvc/src/test/java/org/springframework/web/servlet/SensitiveMvcSupportTests.java"
)
if ($Level -eq 1) {
	$FixtureTargets = @($FixtureTargets | Where-Object {
		$_ -notmatch "SensitiveMethodValidation|spring-webmvc|spring-webflux"
	})
	$ValidCandidatePaths = @(
		"spring-core/src/main/java/org/springframework/core/annotation/Sensitive.java",
		"spring-core/src/test/java/org/springframework/core/annotation/SensitiveTests.java",
		"spring-beans/src/main/java/org/springframework/beans/SensitiveBeanSupport.java",
		"spring-context/src/main/java/org/springframework/validation/SensitiveSupport.java",
		"spring-context/src/main/java/org/springframework/validation/SensitiveValueContext.java",
		"spring-context/src/test/java/org/springframework/validation/SensitiveSupportTests.java",
		"spring-web/src/main/java/org/springframework/web/bind/SensitiveWebSupport.java",
		"spring-web/src/test/java/org/springframework/web/bind/SensitiveWebSupportTests.java"
	)
}

function Assert-True {
	param([object]$Condition, [string]$Message)
	if (-not [bool]$Condition) {
		throw $Message
	}
}

function Assert-Equal {
	param([object]$Actual, [object]$Expected, [string]$Message)
	if ($Actual -ne $Expected) {
		throw "$Message Expected '$Expected', got '$Actual'."
	}
}

function It {
	param([string]$Name, [scriptblock]$Body)
	& $Body
	Write-Host "PASS: $Name"
}

function New-File {
	param([string]$Path, [string]$Content = "class Placeholder {}")
	$directory = Split-Path -Parent $Path
	New-Item -ItemType Directory -Force -Path $directory | Out-Null
	Set-Content -LiteralPath $Path -Value $Content -NoNewline -Encoding utf8
}

$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("spring-sensitive-value-redaction-" + [guid]::NewGuid().ToString("N"))
$Worktree = Join-Path $TempRoot "spring-framework"
$GradleLog = Join-Path $TempRoot "gradle-arguments.log"

function Initialize-Worktree {
	New-Item -ItemType Directory -Force -Path $Worktree | Out-Null
	& git -C $Worktree init --quiet
	& git -C $Worktree config user.email "agent-bench@example.test"
	& git -C $Worktree config user.name "Agent Bench"
	$wrapper = Join-Path $Worktree "gradlew.bat"
	New-File $wrapper "@echo off`r`necho %* >> `"%GRADLE_ARGUMENT_LOG%`"`r`nif not `"%GRADLE_CREATED_DIRTY_PATH%`"==`"`" echo generated > `"%GRADLE_CREATED_DIRTY_PATH%`"`r`nexit /b %GRADLE_EXIT_CODE%`r`n"
	& git -C $Worktree add -- gradlew.bat
	& git -C $Worktree commit --quiet -m "baseline"
	foreach ($relativePath in $ValidCandidatePaths) {
		New-File (Join-Path $Worktree $relativePath)
	}
}

function Invoke-GraderFixture {
	param(
		[int]$GradleExitCode,
		[string]$ExtraDirtyPath,
		[string]$GradleCreatedDirtyPath
	)

	if (-not (Test-Path -LiteralPath $Grader -PathType Leaf)) {
		throw "Grader missing: $Grader"
	}
	if ($ExtraDirtyPath) {
		New-File (Join-Path $Worktree $ExtraDirtyPath) "pluginManagement {}"
	}
	$env:GRADLE_EXIT_CODE = $GradleExitCode
	$env:GRADLE_ARGUMENT_LOG = $GradleLog
	$env:GRADLE_CREATED_DIRTY_PATH = $GradleCreatedDirtyPath
	$output = & $PowerShell -NoProfile -File $Grader -Worktree $Worktree 2>&1
	$arguments = if (Test-Path -LiteralPath $GradleLog) {
		Get-Content -LiteralPath $GradleLog
	}
	else {
		@()
	}
	$remaining = @(
		foreach ($target in $FixtureTargets) {
			if (Test-Path -LiteralPath (Join-Path $Worktree $target)) {
				$target
			}
		}
	)
	return @{
		ExitCode = $LASTEXITCODE
		Output = ($output -join "`n")
		GradleArguments = @($arguments)
		RemainingAgentBenchFiles = @($remaining)
	}
}

try {
	Initialize-Worktree

	It "keeps held-out fixtures on module-local test APIs" {
		foreach ($fixture in Get-ChildItem -LiteralPath $FixtureRoot -Recurse -Filter "*.java") {
			$content = Get-Content -LiteralPath $fixture.FullName -Raw
			Assert-True ($content -notmatch "org\.springframework\.test\.web") `
				"Fixture must not depend on the unavailable spring-test module: $($fixture.FullName)"
			Assert-True ($content -notmatch "addValidators\(\(") `
				"Fixture must use Validator.forInstanceOf(...) instead of treating Validator as functional: $($fixture.FullName)"
			Assert-True ($content -notmatch "new MutablePropertyValues\([^)]*,") `
				"Fixture must use a supported MutablePropertyValues constructor: $($fixture.FullName)"
		}
	}

	It "copies every held-out fixture and removes it after PASS" {
		$result = Invoke-GraderFixture -GradleExitCode 0
		Assert-Equal $result.ExitCode 0 "Expected grader to pass."
		Assert-Equal @($result.RemainingAgentBenchFiles).Count 0 "Expected no held-out fixture files after PASS."
		Assert-True ($result.GradleArguments | Where-Object { $_ -match ":spring-core:test" }) "Expected spring-core Gradle task."
		Assert-True ($result.GradleArguments | Where-Object { $_ -match ":spring-context:test" }) "Expected spring-context Gradle task."
		Assert-True ($result.GradleArguments | Where-Object { $_ -match ":spring-web:test" }) "Expected spring-web Gradle task."
		if ($Level -eq 1) {
			Assert-True (-not ($result.GradleArguments | Where-Object { $_ -match ":spring-webmvc:test|:spring-webflux:test|SensitiveMethodValidationAgentBenchTests" })) `
				"Level 1 must exclude method-validation, MVC, and WebFlux fixtures."
		}
		else {
			Assert-True ($result.GradleArguments | Where-Object { $_ -match ":spring-webmvc:test" }) "Expected spring-webmvc Gradle task."
			Assert-True ($result.GradleArguments | Where-Object { $_ -match ":spring-webflux:test" }) "Expected spring-webflux Gradle task."
		}
	}

	It "fails before Gradle when a candidate path is outside policy" {
		$result = Invoke-GraderFixture -GradleExitCode 0 -ExtraDirtyPath "settings.gradle"
		Assert-True ($result.ExitCode -ne 0) "Expected grader to reject an out-of-policy path."
		Assert-True ($result.Output -match "Protocol violation") "Expected protocol-violation output."
	}
}
finally {
	Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("spring-sensitive-value-redaction-" + [guid]::NewGuid().ToString("N"))
$Worktree = Join-Path $TempRoot "spring-framework"
$GradleLog = Join-Path $TempRoot "gradle-arguments.log"

try {
	Initialize-Worktree

	It "removes held-out fixtures after Gradle failure" {
		$result = Invoke-GraderFixture -GradleExitCode 1
		Assert-True ($result.ExitCode -ne 0) "Expected grader to fail when Gradle fails."
		Assert-Equal @($result.RemainingAgentBenchFiles).Count 0 "Expected no held-out fixture files after Gradle failure."
	}

	It "fails after Gradle when cleanup detects an out-of-policy file" {
		$result = Invoke-GraderFixture -GradleExitCode 0 -GradleCreatedDirtyPath "settings.gradle"
		Assert-True (Test-Path -LiteralPath (Join-Path $Worktree "settings.gradle")) "Expected fake Gradle to create the out-of-policy file."
		Assert-True ($result.ExitCode -ne 0) "Expected post-cleanup policy failure to fail the grader."
		Assert-True ($result.Output -match "Protocol violation") "Expected protocol-violation output."
		Assert-Equal @($result.RemainingAgentBenchFiles).Count 0 "Expected no held-out fixture files after cleanup-policy failure."
	}
}
finally {
	Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
