param(
    [Parameter(Mandatory = $true)]
    [string] $GcalRoot,
    [string] $GoldeneyeBin
)

$arguments = @(
    (Join-Path $PSScriptRoot 'gcal-acceptance.mjs'),
    '--gcal-root',
    $GcalRoot
)
if ($GoldeneyeBin) {
    $arguments += @('--goldeneye-bin', $GoldeneyeBin)
}

& node @arguments
exit $LASTEXITCODE
