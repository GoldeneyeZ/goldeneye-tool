$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
foreach ($name in @('goldeneye.exe', 'LICENSE', 'NOTICE')) {
  $path = Join-Path $toolsDir $name
  if (Test-Path -LiteralPath $path) {
    Remove-Item -LiteralPath $path -Force
  }
}
