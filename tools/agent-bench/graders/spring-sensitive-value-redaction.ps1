[CmdletBinding()]
param(
	[Parameter(Mandatory)]
	[string]$Worktree
)

$ErrorActionPreference = "Stop"
$ExitCode = 1
$Fixtures = @(
	@{ Module = "spring-core"; Source = "spring-core/SensitiveAnnotationAgentBenchTests.java";
		Target = "spring-core/src/test/java/org/springframework/core/annotation/SensitiveAnnotationAgentBenchTests.java";
		Test = "org.springframework.core.annotation.SensitiveAnnotationAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveDataBinderAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/SensitiveDataBinderAgentBenchTests.java";
		Test = "org.springframework.validation.SensitiveDataBinderAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveMethodValidationAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/beanvalidation/SensitiveMethodValidationAgentBenchTests.java";
		Test = "org.springframework.validation.beanvalidation.SensitiveMethodValidationAgentBenchTests" },
	@{ Module = "spring-web"; Source = "spring-web/SensitiveWebBindingInitializerAgentBenchTests.java";
		Target = "spring-web/src/test/java/org/springframework/web/bind/support/SensitiveWebBindingInitializerAgentBenchTests.java";
		Test = "org.springframework.web.bind.support.SensitiveWebBindingInitializerAgentBenchTests" },
	@{ Module = "spring-webmvc"; Source = "spring-webmvc/SensitiveMvcAgentBenchTests.java";
		Target = "spring-webmvc/src/test/java/org/springframework/web/servlet/mvc/method/annotation/SensitiveMvcAgentBenchTests.java";
		Test = "org.springframework.web.servlet.mvc.method.annotation.SensitiveMvcAgentBenchTests" },
	@{ Module = "spring-webflux"; Source = "spring-webflux/SensitiveWebFluxAgentBenchTests.java";
		Target = "spring-webflux/src/test/java/org/springframework/web/reactive/result/method/annotation/SensitiveWebFluxAgentBenchTests.java";
		Test = "org.springframework.web.reactive.result.method.annotation.SensitiveWebFluxAgentBenchTests" }
)
$AllowedPrefixes = @(
	"spring-core/src/",
	"spring-beans/src/",
	"spring-context/src/",
	"spring-web/src/",
	"spring-webmvc/src/",
	"spring-webflux/src/"
)
$RequiredPrefixes = @(
	"spring-core/src/main/java/",
	"spring-context/src/main/java/",
	"spring-web/src/main/java/"
)
$FixtureRoot = Join-Path $PSScriptRoot "fixtures/spring-sensitive-value-redaction"

function Get-CandidateDirtyFiles {
	param([string]$Repository)
	$tracked = @(& git -C $Repository diff --name-only --no-renames HEAD)
	$untracked = @(& git -C $Repository ls-files --others --exclude-standard)
	return @($tracked + $untracked | Where-Object { $_ } | Sort-Object -Unique)
}

function Assert-AllowedCandidateFiles {
	param([string]$Repository)
	$actual = @(Get-CandidateDirtyFiles $Repository)
	$disallowed = @($actual | Where-Object {
		$path = $_
		-not ($AllowedPrefixes | Where-Object { $path.StartsWith($_, [System.StringComparison]::Ordinal) })
	})
	$missingRequiredPrefixes = @($RequiredPrefixes | Where-Object {
		$requiredPrefix = $_
		-not ($actual | Where-Object { $_.StartsWith($requiredPrefix, [System.StringComparison]::Ordinal) })
	})
	if ($actual.Count -lt 8 -or $actual.Count -gt 40 -or $disallowed -or $missingRequiredPrefixes) {
		throw "Protocol violation: candidate changes must use the six Spring module policy (8-40 paths); found $($actual -join ', '); disallowed $($disallowed -join ', '); missing required prefixes $($missingRequiredPrefixes -join ', ')."
	}
}

try {
	$Worktree = (Resolve-Path -LiteralPath $Worktree -ErrorAction Stop).Path
	$GradleWrapper = Join-Path $Worktree "gradlew.bat"
	if (-not (Test-Path -LiteralPath $GradleWrapper -PathType Leaf)) {
		throw "Required Spring path is missing: $GradleWrapper"
	}
	foreach ($fixture in $Fixtures) {
		$source = Join-Path $FixtureRoot $fixture.Source
		$target = Join-Path $Worktree $fixture.Target
		if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
			throw "Held-out fixture is missing: $source"
		}
		if (Test-Path -LiteralPath $target) {
			throw "Held-out test collision: $target must be absent before grading"
		}
	}

	Assert-AllowedCandidateFiles $Worktree
	try {
		foreach ($fixture in $Fixtures) {
			$source = Join-Path $FixtureRoot $fixture.Source
			$target = Join-Path $Worktree $fixture.Target
			New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
			Copy-Item -LiteralPath $source -Destination $target
		}

		foreach ($group in ($Fixtures | Group-Object Module)) {
			$arguments = @(
				":$($group.Name):test"
			)
			foreach ($fixture in $group.Group) {
				$arguments += "--tests"
				$arguments += $fixture.Test
			}
			$arguments += "--build-cache"
			Push-Location -LiteralPath $Worktree
			try {
				& $GradleWrapper @arguments
				$GradleExitCode = $LASTEXITCODE
			}
			finally {
				Pop-Location
			}
			if ($GradleExitCode -ne 0) {
				throw "Held-out Spring tests for $($group.Name) failed with exit code $GradleExitCode"
			}
		}
		$ExitCode = 0
	}
	finally {
		foreach ($fixture in $Fixtures) {
			$target = Join-Path $Worktree $fixture.Target
			if (Test-Path -LiteralPath $target) {
				Remove-Item -LiteralPath $target -Force
			}
		}
		$DiffCheckOutput = @(& git -C $Worktree diff --check HEAD 2>&1)
		if ($LASTEXITCODE -ne 0) {
			throw "Post-cleanup whitespace verification failed: $($DiffCheckOutput -join ' ')"
		}
		Assert-AllowedCandidateFiles $Worktree
	}
}
catch {
	Write-Error $_
}

exit $ExitCode
