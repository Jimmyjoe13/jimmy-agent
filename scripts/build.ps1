# Compile Jimmy (mode debug ou release).
#
#   .\scripts\build.ps1            # debug
#   .\scripts\build.ps1 -Release   # release + installateur
#
# Passe par with-msvc.ps1 : Rust cible MSVC, le linker vient des Visual Studio
# Build Tools qui fournissent leur propre environnement.

[CmdletBinding()]
param(
    [switch]$Release,
    # Produit en plus l'installateur NSIS. Plus long, et demande NSIS.
    [switch]$Bundles
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($Release) {
    Push-Location (Join-Path $root 'desktop')

    if ($Bundles) {
        # Installateur Windows complet (nécessite NSIS, téléchargé au besoin).
        & (Join-Path $root 'scripts\with-msvc.ps1') npx tauri build --bundles nsis
        $code = $LASTEXITCODE
        Pop-Location
        exit $code
    }

    # Binaire autonome pour le raccourci Bureau.
    #
    # `tauri build` et non `cargo build --release` : c'est la CLI Tauri qui
    # embarque les assets du frontend dans le binaire. Avec `cargo build` seul,
    # la fenêtre en release continue de viser `devUrl` et affiche « Impossible
    # d'accéder à cette page » — le binaire démarre, mais sans interface.
    & (Join-Path $root 'scripts\with-msvc.ps1') npx tauri build --no-bundle
    $code = $LASTEXITCODE
    Pop-Location
    exit $code
}

Write-Host "Compilation debug…" -ForegroundColor Cyan
& (Join-Path $root 'scripts\with-msvc.ps1') cargo build --message-format short
exit $LASTEXITCODE