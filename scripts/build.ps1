# Compile Jimmy (mode debug ou release).
#
#   .\scripts\build.ps1            # debug
#   .\scripts\build.ps1 -Release   # release + installateur
#
# Passe par with-msvc.ps1 : Rust cible MSVC, le linker vient des Visual Studio
# Build Tools qui fournissent leur propre environnement.

[CmdletBinding()]
param([switch]$Release)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($Release) {
    Write-Host "Compilation release…" -ForegroundColor Cyan
    & (Join-Path $root 'scripts\with-msvc.ps1') cargo build --release --message-format short
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Push-Location (Join-Path $root 'desktop')
    npm run build | Out-Host
    if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
    & (Join-Path $root 'scripts\with-msvc.ps1') npx tauri build --bundles nsis
    $code = $LASTEXITCODE
    Pop-Location
    exit $code
}

Write-Host "Compilation debug…" -ForegroundColor Cyan
& (Join-Path $root 'scripts\with-msvc.ps1') cargo build --message-format short
exit $LASTEXITCODE