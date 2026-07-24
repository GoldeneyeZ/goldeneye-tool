[CmdletBinding()]
param(
	[string]$Repo = "D:\Dev\IdeaProjects\spring-framework",
	[string]$BaseRef = "daf955157871e4ac6f192e06b71d6cc595eb979b",
	[string]$Grader = (Join-Path $PSScriptRoot "spring-stringutils-unicode-truncate.ps1")
)

$ErrorActionPreference = "Stop"
$PowerShell = (Get-Command pwsh -ErrorAction Stop).Source
$SourceRelativePath = "spring-core/src/main/java/org/springframework/util/StringUtils.java"
$TestsRelativePath = "spring-core/src/test/java/org/springframework/util/StringUtilsTests.java"
$HeldOutRelativePath = "spring-core/src/test/java/org/springframework/util/AgentBenchStringUtilsUnicodeTests.java"
$UnexpectedRelativePath = "spring-core/src/test/java/org/springframework/util/AgentBenchUnexpected.java"
$OriginalRepoStatus = (& git -C $Repo status --porcelain) -join "`n"
$Worktree = "D:\s-" + [guid]::NewGuid().ToString("N").Substring(0, 8)

function Assert-Equal {
	param([object]$Actual, [object]$Expected, [string]$Message)
	if ($Actual -ne $Expected) {
		throw "$Message Expected '$Expected', got '$Actual'."
	}
}

function Assert-True {
	param([bool]$Condition, [string]$Message)
	if (-not $Condition) {
		throw $Message
	}
}

function Invoke-Grader {
	param([string]$ExpectedCase)
	if (-not (Test-Path -LiteralPath $Grader -PathType Leaf)) {
		throw "Grader missing: $Grader"
	}
	$output = & $PowerShell -NoProfile -File $Grader -Worktree $Worktree 2>&1
	return @{ ExitCode = $LASTEXITCODE; Output = ($output -join "`n") }
}

function Set-CaseFiles {
	param([string]$Implementation, [bool]$ChangeTests, [bool]$AddTrailingWhitespace = $false)
	$source = $OriginalSource.Replace($OldImplementation, $Implementation)
	if ($Implementation -ne $OldImplementation) {
		Assert-True ($source -ne $OriginalSource) "Controlled source patch did not apply."
	}
	Set-Content -LiteralPath $SourcePath -Value $source -NoNewline -Encoding utf8
	$tests = $OriginalTests
	if ($ChangeTests) {
		$tests += "`n// Controlled grader protocol patch.`n"
	}
	if ($AddTrailingWhitespace) {
		$tests += "`n// Controlled grader whitespace patch. `n"
	}
	Set-Content -LiteralPath $TestsPath -Value $tests -NoNewline -Encoding utf8
}

function Assert-HeldOutCleanup {
	Assert-True (-not (Test-Path -LiteralPath $HeldOutPath)) "Held-out test remained after grader outcome."
}

try {
	Assert-True (Test-Path -LiteralPath $Repo -PathType Container) "Spring repository is missing: $Repo"
	Assert-Equal ((& git -C $Repo rev-parse HEAD).Trim()) $BaseRef "Spring repository HEAD differs from pinned base."
	Assert-Equal $OriginalRepoStatus "" "Spring source repository must begin clean."
	& git -C $Repo worktree add --detach $Worktree $BaseRef
	if ($LASTEXITCODE -ne 0) { throw "Could not create disposable Spring worktree." }

	$SourcePath = Join-Path $Worktree $SourceRelativePath
	$TestsPath = Join-Path $Worktree $TestsRelativePath
	$HeldOutPath = Join-Path $Worktree $HeldOutRelativePath
	$UnexpectedPath = Join-Path $Worktree $UnexpectedRelativePath
	$OriginalSource = Get-Content -LiteralPath $SourcePath -Raw
	$OriginalTests = Get-Content -LiteralPath $TestsPath -Raw
	$OldImplementation = [regex]::Match(
		$OriginalSource,
		'(?s)\tpublic static String truncate\(CharSequence charSequence, int threshold\) \{.*?\r?\n\t\}'
	).Value
	Assert-True ($OldImplementation.Length -gt 0) "Could not locate pinned truncate implementation."
	$SafeImplementation = @'
	public static String truncate(CharSequence charSequence, int threshold) {
		Assert.isTrue(threshold > 0,
				() -> "Truncation threshold must be a positive number: " + threshold);
		if (charSequence.length() > threshold) {
			int prefixLength = threshold;
			if (Character.isHighSurrogate(charSequence.charAt(threshold - 1)) &&
					Character.isLowSurrogate(charSequence.charAt(threshold))) {
				prefixLength--;
			}
			return charSequence.subSequence(0, prefixLength) + TRUNCATION_SUFFIX;
		}
		return charSequence.toString();
	}
'@.Replace("`n", [Environment]::NewLine)
	$NoOpOldImplementation = $OldImplementation.Replace(
		"return charSequence.toString();",
		("// Controlled old implementation patch." + [Environment]::NewLine + "`t`treturn charSequence.toString();")
	)
	$BadSuffixImplementation = $SafeImplementation.Replace("TRUNCATION_SUFFIX", '" [changed]"')
	$BadPreconditionImplementation = $SafeImplementation.Replace("threshold > 0", "threshold >= 0")

	Set-CaseFiles $NoOpOldImplementation $true
	$result = Invoke-Grader "old implementation"
	Assert-Equal $result.ExitCode 1 "Old implementation must fail held-out test."
	Assert-HeldOutCleanup

	Set-CaseFiles $SafeImplementation $true
	$result = Invoke-Grader "boundary-safe implementation"
	Assert-Equal $result.ExitCode 0 "Boundary-safe implementation must pass. Output: $($result.Output)"
	Assert-HeldOutCleanup

	Set-CaseFiles $SafeImplementation $true
	Set-Content -LiteralPath $UnexpectedPath -Value "package org.springframework.util; class AgentBenchUnexpected {}" -NoNewline -Encoding utf8
	$result = Invoke-Grader "unexpected candidate path"
	Assert-Equal $result.ExitCode 1 "Unexpected candidate path must fail protocol."
	Assert-HeldOutCleanup
	Remove-Item -LiteralPath $UnexpectedPath -Force

	Set-CaseFiles $BadSuffixImplementation $true
	$result = Invoke-Grader "changed suffix"
	Assert-Equal $result.ExitCode 1 "Changed suffix must fail held-out test."
	Assert-HeldOutCleanup

	Set-CaseFiles $BadPreconditionImplementation $true
	$result = Invoke-Grader "changed precondition"
	Assert-Equal $result.ExitCode 1 "Changed precondition must fail held-out test."
	Assert-HeldOutCleanup

	Set-CaseFiles $SafeImplementation $false
	$result = Invoke-Grader "missing focused test change"
	Assert-Equal $result.ExitCode 1 "Missing StringUtilsTests.java change must fail protocol."
	Assert-HeldOutCleanup

	Set-CaseFiles $SafeImplementation $true
	$CollisionContents = "// Existing held-out test must remain untouched.`n"
	Set-Content -LiteralPath $HeldOutPath -Value $CollisionContents -NoNewline -Encoding utf8
	$result = Invoke-Grader "existing held-out test"
	Assert-Equal $result.ExitCode 1 "Existing held-out test must fail protocol."
	Assert-Equal (Get-Content -LiteralPath $HeldOutPath -Raw) $CollisionContents "Existing held-out test was modified."
	Remove-Item -LiteralPath $HeldOutPath -Force

	Set-CaseFiles $SafeImplementation $true $true
	$result = Invoke-Grader "trailing whitespace"
	Assert-Equal $result.ExitCode 1 "Trailing whitespace must fail post-cleanup verification."
	Assert-HeldOutCleanup
}
finally {
	if (Test-Path -LiteralPath $Worktree) {
		& git -C $Repo worktree remove --force $Worktree
		if (Test-Path -LiteralPath $Worktree) {
			Remove-Item -LiteralPath $Worktree -Recurse -Force
		}
	}
	Assert-Equal ((& git -C $Repo status --porcelain) -join "`n") $OriginalRepoStatus "Grader self-test modified pinned Spring source repository."
}

Write-Host "PASS: Spring StringUtils Unicode grader self-test"
