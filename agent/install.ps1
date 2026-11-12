[CmdletBinding()]
param(
  [switch]$SkipBuild,
  [switch]$SkipGlobalLink,
  [switch]$SkipSkills,
  [string]$GoldeneyeCommand,
  [switch]$Force
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Write-Step {
  param([Parameter(Mandatory = $true)][string]$Message)

  Write-Host "==> $Message"
}

function Assert-Command {
  param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$InstallHint
  )

  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "$Name is required. $InstallHint"
  }
}

function Assert-NodeVersion {
  Assert-Command "node" "Install Node.js 20 or newer, then re-run this installer."

  $nodeVersion = (& node --version).Trim()
  if ($nodeVersion -notmatch '^v(?<major>\d+)\.') {
    throw "Could not parse Node.js version from '$nodeVersion'."
  }

  $major = [int]$Matches["major"]
  if ($major -lt 20) {
    throw "Node.js 20 or newer is required. Found $nodeVersion."
  }
}

function Resolve-GoldeneyeCommand {
  param([string]$RequestedCommand)

  $candidates = @()
  if ($RequestedCommand) {
    $candidates += $RequestedCommand
  }

  $configuredCommand = [Environment]::GetEnvironmentVariable("GCAL_GOLDENEYE_COMMAND", "User")
  if ($configuredCommand) {
    $candidates += $configuredCommand
  }

  $agentRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
  $goldeneyeRoot = Split-Path -Parent $agentRoot
  $candidates += Join-Path $goldeneyeRoot "target\release\goldeneye.exe"

  $legacyCommand = [Environment]::GetEnvironmentVariable("GCAL_MCP_COMMAND", "User")
  if ($legacyCommand -and [IO.Path]::GetFileName($legacyCommand) -match '^goldeneye(\.exe)?$') {
    $candidates += $legacyCommand
  }

  $goldeneyeOnPath = Get-Command "goldeneye" -ErrorAction SilentlyContinue
  if ($goldeneyeOnPath) {
    $candidates += $goldeneyeOnPath.Source
  }

  $candidates += Join-Path $HOME "IdeaProjects\goldeneye-tool\target\release\goldeneye.exe"
  foreach ($drive in Get-PSDrive -PSProvider FileSystem) {
    $candidates += Join-Path $drive.Root "Dev\IdeaProjects\goldeneye-tool\target\release\goldeneye.exe"
  }

  foreach ($candidate in $candidates) {
    if (-not $candidate) { continue }

    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
      return [IO.Path]::GetFullPath($candidate)
    }

    $resolvedCommand = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($resolvedCommand) {
      return $resolvedCommand.Source
    }
  }

  throw "Goldeneye was not found. Build or install Goldeneye, add it to PATH, or pass -GoldeneyeCommand <path>."
}

function Invoke-Native {
  param(
    [Parameter(Mandatory = $true)][string]$Display,
    [Parameter(Mandatory = $true)][scriptblock]$Command
  )

  Write-Host "+ $Display"
  & $Command

  if ($LASTEXITCODE -ne 0) {
    throw "$Display failed with exit code $LASTEXITCODE."
  }
}

function Set-ManagedBlock {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Block
  )

  $startMarker = "<!-- gcal-installer:start -->"
  $endMarker = "<!-- gcal-installer:end -->"
  $pattern = "(?s)$([regex]::Escape($startMarker)).*?$([regex]::Escape($endMarker))"

  if (Test-Path -LiteralPath $Path) {
    $existing = Get-Content -LiteralPath $Path -Raw
    if ($existing -match $pattern) {
      $updated = [regex]::Replace(
        $existing,
        $pattern,
        [System.Text.RegularExpressions.MatchEvaluator] { param($match) $Block }
      )
    } else {
      $updated = $existing.TrimEnd() + [Environment]::NewLine + [Environment]::NewLine + $Block
    }
  } else {
    $updated = $Block
  }

  Set-Content -LiteralPath $Path -Value $updated -Encoding utf8
}

function Remove-ManagedBlock {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$StartMarker,
    [Parameter(Mandatory = $true)][string]$EndMarker
  )

  if (-not (Test-Path -LiteralPath $Path)) { return }

  $existing = Get-Content -LiteralPath $Path -Raw
  $pattern = "(?s)$([regex]::Escape($StartMarker)).*?$([regex]::Escape($EndMarker))\s*"
  $updated = [regex]::Replace($existing, $pattern, "").TrimEnd()
  if ($updated.Length -gt 0) { $updated += [Environment]::NewLine }
  Set-Content -LiteralPath $Path -Value $updated -Encoding utf8
}

function Test-LegacyGcalSkill {
  param([Parameter(Mandatory = $true)][string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $false }

  $expectedFiles = @("SKILL.md", "agents/claude.md", "agents/openai.yaml")
  $actualFiles = @(
    Get-ChildItem -LiteralPath $Path -Recurse -File | ForEach-Object {
      [IO.Path]::GetRelativePath($Path, $_.FullName).Replace("\", "/")
    }
  )
  if ($actualFiles.Count -ne $expectedFiles.Count) { return $false }
  foreach ($expectedFile in $expectedFiles) {
    if ($actualFiles -notcontains $expectedFile) { return $false }
  }

  $skillMd = Get-Content -LiteralPath (Join-Path $Path "SKILL.md") -Raw
  return $skillMd -match '(?m)^name:\s*codebase-memory\s*$' -and $skillMd -match 'GCAL'
}

$repoRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
$codexRoot = Join-Path $HOME ".codex"
$skillsRoot = Join-Path $codexRoot "skills"
$sourceSkillRelative = "workflow/skills/goldeneye-code-agent-layer"
$sourceSkill = Join-Path $repoRoot $sourceSkillRelative
$destinationSkillRelative = ".codex/skills/goldeneye-code-agent-layer"
$destinationSkill = Join-Path $skillsRoot "goldeneye-code-agent-layer"
$legacyDestinationSkill = Join-Path $skillsRoot "codebase-memory"
$sourceAgents = Join-Path $repoRoot "workflow/AGENTS.md"
$codexAgents = Join-Path $codexRoot "AGENTS.md"

Write-Step "Checking prerequisites"
Assert-NodeVersion
Assert-Command "pnpm" "Install pnpm, or run 'corepack enable' from an elevated shell if Node installed Corepack."
$resolvedGoldeneyeCommand = Resolve-GoldeneyeCommand -RequestedCommand $GoldeneyeCommand

Push-Location $repoRoot
try {
  Write-Step "Installing dependencies"
  Invoke-Native "pnpm install" { & pnpm install }

  if ($SkipBuild) {
    Write-Step "Skipping build because -SkipBuild was supplied"
  } else {
    Write-Step "Building GCAL"
    Invoke-Native "pnpm build" { & pnpm build }
  }

  if ($SkipGlobalLink) {
    Write-Step "Skipping global installation because -SkipGlobalLink was supplied"
  } else {
    Write-Step "Installing gcal globally"
    Invoke-Native "pnpm add --global --allow-build=esbuild $repoRoot" {
      & pnpm add --global --allow-build=esbuild $repoRoot
    }
  }
} finally {
  Pop-Location
}

Write-Step "Configuring Goldeneye as GCAL's default backend"
[Environment]::SetEnvironmentVariable("GCAL_BACKEND", "goldeneye", "User")
[Environment]::SetEnvironmentVariable("GCAL_GOLDENEYE_COMMAND", $resolvedGoldeneyeCommand, "User")
[Environment]::SetEnvironmentVariable("GCAL_MCP_COMMAND", $null, "User")
[Environment]::SetEnvironmentVariable("GCAL_MCP_URL", $null, "User")
$env:GCAL_BACKEND = "goldeneye"
$env:GCAL_GOLDENEYE_COMMAND = $resolvedGoldeneyeCommand
Remove-Item Env:GCAL_MCP_COMMAND -ErrorAction SilentlyContinue
Remove-Item Env:GCAL_MCP_URL -ErrorAction SilentlyContinue
Write-Host "Goldeneye command: $resolvedGoldeneyeCommand"

if ($SkipSkills) {
  Write-Step "Skipping Codex skill installation because -SkipSkills was supplied"
} else {
  Write-Step "Installing Codex skill assets"
  if (-not (Test-Path -LiteralPath $sourceSkill)) {
    throw "Missing source skill directory: $sourceSkillRelative"
  }

  New-Item -ItemType Directory -Path $destinationSkill -Force | Out-Null

  if ($Force -and (Test-Path -LiteralPath $destinationSkill)) {
    Remove-Item -LiteralPath $destinationSkill -Recurse -Force
    New-Item -ItemType Directory -Path $destinationSkill -Force | Out-Null
  }

  Copy-Item -Path (Join-Path $sourceSkill "*") -Destination $destinationSkill -Recurse -Force
  $installedSkillMd = Join-Path $destinationSkill "SKILL.md"
  if (-not (Test-Path -LiteralPath $installedSkillMd) -or
      (Get-Content -LiteralPath $installedSkillMd -Raw) -notmatch '(?m)^name:\s*goldeneye-code-agent-layer\s*$') {
    throw "Installed skill validation failed: $destinationSkillRelative"
  }
  Write-Host "Installed skill to $destinationSkillRelative"

  if (Test-LegacyGcalSkill -Path $legacyDestinationSkill) {
    $resolvedSkillsRoot = [IO.Path]::GetFullPath($skillsRoot)
    $resolvedLegacySkill = [IO.Path]::GetFullPath($legacyDestinationSkill)
    if (-not $resolvedLegacySkill.StartsWith($resolvedSkillsRoot, [StringComparison]::OrdinalIgnoreCase)) {
      throw "Legacy skill path escapes the Codex skills directory: $resolvedLegacySkill"
    }
    Remove-Item -LiteralPath $resolvedLegacySkill -Recurse -Force
    Write-Host "Removed legacy GCAL skill .codex/skills/codebase-memory"
  } elseif (Test-Path -LiteralPath $legacyDestinationSkill) {
    Write-Warning "Preserved unrecognized legacy skill directory: .codex/skills/codebase-memory"
  }

  if (Test-Path -LiteralPath $sourceAgents) {
    New-Item -ItemType Directory -Path $codexRoot -Force | Out-Null
    $workflowRules = Get-Content -LiteralPath $sourceAgents -Raw
    $managedBlock = @"
<!-- gcal-installer:start -->
$workflowRules
<!-- gcal-installer:end -->
"@
    Remove-ManagedBlock `
      -Path $codexAgents `
      -StartMarker "<!-- codebase-memory-mcp:start -->" `
      -EndMarker "<!-- codebase-memory-mcp:end -->"
    Set-ManagedBlock -Path $codexAgents -Block $managedBlock
    Write-Host "Updated GCAL workflow rules in $codexAgents"
  }
}

Write-Step "Installation finished"
Write-Host "Run 'gcal --help' to verify the global command."
