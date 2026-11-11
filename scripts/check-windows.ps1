# Run from the repository root on Windows with Rust 1.95+.
$ErrorActionPreference = 'Stop'

$checks = @(
    @{ title = 'Rust formatting'; args = @('fmt','--all','--','--check') },
    @{ title = 'Unit tests'; args = @('test','--locked') },
    @{ title = 'Release build'; args = @('build','--release','--locked') }
)

foreach ($check in $checks) {
    Write-Host ''
    Write-Host "== $($check.title) ==" -ForegroundColor Cyan
    & cargo @($check.args)
    if ($LASTEXITCODE -ne 0) {
        throw "$($check.title) failed: exit code $LASTEXITCODE"
    }
}
Write-Host ''
Write-Host 'Formatting, tests and release compilation passed.' -ForegroundColor Green
Write-Host 'Manual Windows GUI and mouse checks are still required.'
