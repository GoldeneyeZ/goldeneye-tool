param(
    [string] $GoldeneyeBin
)

$arguments = @((Join-Path $PSScriptRoot 'edit-acceptance.mjs'))
if ($GoldeneyeBin) {
    $arguments += @('--goldeneye-bin', $GoldeneyeBin)
}

& node @arguments
exit $LASTEXITCODE
