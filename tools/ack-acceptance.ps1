param(
    [Parameter(Mandatory = $true)]
    [string] $AckRoot,
    [string] $GoldeneyeBin
)

$arguments = @(
    (Join-Path $PSScriptRoot 'ack-acceptance.mjs'),
    '--ack-root',
    $AckRoot
)
if ($GoldeneyeBin) {
    $arguments += @('--goldeneye-bin', $GoldeneyeBin)
}

& node @arguments
exit $LASTEXITCODE
