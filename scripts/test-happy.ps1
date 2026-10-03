# Lance le test du chemin heureux (il consomme de vrais services).
#
#   .\scripts\test-happy.ps1
#
# Nécessite un `.env` à la racine avec OPENCODE_API_KEY.
param(
    [switch]$SkipAudio
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$env:RUST_LOG = 'jimmy_agent=info'

# Test de l'agent : demande réelle, outils réels.
& (Join-Path $root 'scripts\with-msvc.ps1') cmd /c 'cargo test -p jimmy-agent --test happy_path -- --ignored --nocapture'
$code = $LASTEXITCODE

if (-not $SkipAudio) {
    Write-Host "`n--- Chaîne audio (TTS et écoute permanente) ---" -ForegroundColor Cyan
    & (Join-Path $root 'scripts\with-msvc.ps1') cmd /c 'cargo test -p jimmy-agent --test audio -- --ignored --nocapture --test-threads=1'
    if ($LASTEXITCODE -ne 0) { $code = $LASTEXITCODE }
}

exit $code