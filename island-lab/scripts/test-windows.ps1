# Island Lab: local Windows development smoke test.
# Execute from PowerShell after installing Rust (MSVC), Node.js and C++ Build Tools.
# This script never modifies the main Snowy New Year application.
$ErrorActionPreference = 'Stop'

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$native = (Resolve-Path (Join-Path $scriptRoot '..\native')).Path
Push-Location $native
try {
    Write-Host 'Island Lab | Native Windows smoke test' -ForegroundColor Cyan
    foreach ($command in @('node', 'npm', 'cargo')) {
        if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
            throw "Required command missing: $command. See island-lab/native/README.md."
        }
    }

    Write-Host 'Node:' (node --version)
    Write-Host 'Cargo:' (cargo --version)

    Write-Host '1/3 Install Tauri CLI dependencies...' -ForegroundColor Cyan
    & npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm install failed ($LASTEXITCODE)" }

    Write-Host '2/3 Run Island Lab web smoke tests...' -ForegroundColor Cyan
    & npm test
    if ($LASTEXITCODE -ne 0) { throw "Smoke tests failed ($LASTEXITCODE)" }

    Write-Host '3/3 Build unsigned Windows executable...' -ForegroundColor Cyan
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "Native build failed ($LASTEXITCODE)" }

    $exe = Join-Path $native 'src-tauri\target\release\island-lab-native.exe'
    if (-not (Test-Path $exe)) {
        throw "Build returned success but EXE is missing: $exe"
    }
    Write-Host ''
    Write-Host 'Build successful.' -ForegroundColor Green
    Write-Host "EXE: $exe"
    Write-Host 'The app is still an experimental borderless window, not Explorer desktop docking.'
    Write-Host 'After launching: Settings -> Check -> Copy diagnostics and inspect main/lyrics/screenshot.'
}
finally {
    Pop-Location
}
