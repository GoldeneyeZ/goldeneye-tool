param(
    [string] $GcalRoot,
    [string] $GoldeneyeBin
)

$arguments = @(
    (Join-Path $PSScriptRoot 'gcal-acceptance.mjs')
)
if ($GcalRoot) {
    $arguments += @('--gcal-root', $GcalRoot)
}
if ($GoldeneyeBin) {
    $arguments += @('--goldeneye-bin', $GoldeneyeBin)
}

& node @arguments
exit $LASTEXITCODE
