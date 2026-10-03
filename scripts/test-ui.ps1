# Jimmy — tests de bout en bout de l'interface, sur la vraie application.
#
# Relance Jimmy (release) avec le débogage distant de WebView2, pilote
# l'interface par CDP (Playwright), supprime les sessions de test, puis
# relance Jimmy normalement — le port de débogage ne reste jamais ouvert.
#
#   .\scripts\test-ui.ps1
#
# Prérequis : release compilée (.\scripts\build.ps1 -Release), Node.js.
# Les captures sont écrites dans data\ui-test\.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\jimmy.exe'
$suite = Join-Path $PSScriptRoot 'ui-test'
if (-not (Test-Path $exe)) { throw "Release introuvable : lancer d'abord .\scripts\build.ps1 -Release" }

function Stop-Jimmy {
    Get-Process jimmy -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
    Get-CimInstance Win32_Process -Filter "Name like 'Godot%'" |
        Where-Object { $_.CommandLine -match 'port=8787' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -Confirm:$false }
    Get-Process whisper-server -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
    Start-Sleep -Milliseconds 800
}

if (-not (Test-Path (Join-Path $suite 'node_modules'))) {
    Push-Location $suite
    npm install --silent
    Pop-Location
}

Stop-Jimmy
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
Start-Process -FilePath $exe -WorkingDirectory $root | Out-Null
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
Start-Sleep -Seconds 14
# La fenêtre démarre masquée : on l'ouvre comme le ferait un clic sur l'avatar.
Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:8790/ui/open' -Body '{}' -ContentType 'application/json' | Out-Null
Start-Sleep -Seconds 2

$ErrorActionPreference = 'Continue'
node (Join-Path $suite 'suite.js')
$code = $LASTEXITCODE
node (Join-Path $suite 'cleanup.js')

# Relance normale, sans port de débogage.
Stop-Jimmy
& (Join-Path $PSScriptRoot 'launcher.ps1')
exit $code
