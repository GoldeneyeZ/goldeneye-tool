[CmdletBinding()]
param(
	[Parameter(Mandatory)]
	[string]$Worktree
)

$ErrorActionPreference = "Stop"
$SourceRelativePath = "spring-core/src/main/java/org/springframework/util/StringUtils.java"
$TestsRelativePath = "spring-core/src/test/java/org/springframework/util/StringUtilsTests.java"
$HeldOutRelativePath = "spring-core/src/test/java/org/springframework/util/AgentBenchStringUtilsUnicodeTests.java"
$ExitCode = 1

function Get-CandidateDirtyFiles {
	param([string]$Repository)
	$tracked = @(& git -C $Repository diff --name-only --no-renames HEAD)
	$untracked = @(& git -C $Repository ls-files --others --exclude-standard)
	return @($tracked + $untracked | Where-Object { $_ } | Sort-Object -Unique)
}

function Assert-AllowedCandidateFiles {
	param([string]$Repository, [string[]]$AllowedFiles)
	$actual = Get-CandidateDirtyFiles $Repository
	$unexpected = Compare-Object -ReferenceObject $AllowedFiles -DifferenceObject $actual
	if ($unexpected) {
		throw "Protocol violation: candidate changes must be exactly $($AllowedFiles -join ', '); found $($actual -join ', ')"
	}
}

try {
	$Worktree = (Resolve-Path -LiteralPath $Worktree -ErrorAction Stop).Path
	$SourcePath = Join-Path $Worktree $SourceRelativePath
	$TestsPath = Join-Path $Worktree $TestsRelativePath
	$HeldOutPath = Join-Path $Worktree $HeldOutRelativePath
	$GradleWrapper = Join-Path $Worktree "gradlew.bat"
	foreach ($path in @($SourcePath, $TestsPath, $GradleWrapper)) {
		if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
			throw "Required Spring path is missing: $path"
		}
	}

	if (Test-Path -LiteralPath $HeldOutPath) {
		throw "Held-out test collision: $HeldOutPath must be absent before grading"
	}
	$AllowedCandidateFiles = @($SourceRelativePath, $TestsRelativePath)
	Assert-AllowedCandidateFiles $Worktree $AllowedCandidateFiles

	try {
		$HeldOutTest = @'
package org.springframework.util;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatIllegalArgumentException;

class AgentBenchStringUtilsUnicodeTests {

	@Test
	void truncateDoesNotSplitSurrogatePair() {
		assertThat(StringUtils.truncate("ab\uD83D\uDE00cd", 3)).isEqualTo("ab (truncated)...");
	}

	@Test
	void truncateKeepsCompletePairAtThreshold() {
		assertThat(StringUtils.truncate("ab\uD83D\uDE00cd", 4)).isEqualTo("ab\uD83D\uDE00 (truncated)...");
	}

	@Test
	void truncatePreservesExistingContract() {
		assertThat(StringUtils.truncate(new StringBuilder("abc"), 3)).isEqualTo("abc");
		assertThatIllegalArgumentException()
				.isThrownBy(() -> StringUtils.truncate("abc", 0))
				.withMessage("Truncation threshold must be a positive number: 0");
	}
}
'@
		Set-Content -LiteralPath $HeldOutPath -Value $HeldOutTest -NoNewline -Encoding utf8
		Push-Location -LiteralPath $Worktree
		try {
			& $GradleWrapper ":spring-core:test" "--tests" "org.springframework.util.AgentBenchStringUtilsUnicodeTests" "--build-cache"
			$GradleExitCode = $LASTEXITCODE
		}
		finally {
			Pop-Location
		}
		if ($GradleExitCode -ne 0) {
			throw "Held-out Spring test failed with exit code $GradleExitCode"
		}
		$ExitCode = 0
	}
	finally {
		if (Test-Path -LiteralPath $HeldOutPath) {
			Remove-Item -LiteralPath $HeldOutPath -Force
		}
		$DiffCheckOutput = @(& git -C $Worktree diff --check HEAD 2>&1)
		if ($LASTEXITCODE -ne 0) {
			throw "Post-cleanup whitespace verification failed: $($DiffCheckOutput -join ' ')"
		}
		Assert-AllowedCandidateFiles $Worktree $AllowedCandidateFiles
	}
}
catch {
	Write-Error $_
}

exit $ExitCode
