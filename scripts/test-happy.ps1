# Lance le test du chemin heureux (il consomme de vrais services).
#
#   .\scripts\test-happy.ps1
#
# Nécessite un `.env` à la racine avec OPENCODE_API_KEY.
param([switch]$Keep)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$env:RUST_LOG = 'jimmy_agent=info'
& (Join-Path $root 'scripts\with-msvc.ps1') cmd /c 'cargo test -p jimmy-agent --test happy_path -- --ignored --nocapture'
exit $LASTEXITCODE