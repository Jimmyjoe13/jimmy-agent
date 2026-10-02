# Lance Jimmy en mode développement (Tauri + Vite + Godot).
#
# Le toolchain Rust cible MSVC : le linker vit dans les Visual Studio Build
# Tools, qui fournissent leur propre environnement. Ce script passe par
# `with-msvc.ps1` pour que rien n'ait à être configuré à la main.
param(
    # Passe des arguments à la CLI Tauri (`dev`, `build`, `--` options…).
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args = @('dev')
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $root 'desktop')

if (-not (Test-Path (Join-Path $root 'desktop\node_modules'))) {
    Write-Host "Installation des dépendances front…"
    npm install --no-audit --no-fund
}

if (-not (Test-Path (Join-Path $root '.env'))) {
    Write-Warning "Aucun .env à la racine : copiez .env.example en .env et renseignez vos clés."
}

Write-Host "Démarrage de Jimmy…"
& (Join-Path $root 'scripts\with-msvc.ps1') npx tauri @Args
exit $LASTEXITCODE