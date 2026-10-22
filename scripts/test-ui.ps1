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
# Interrupteurs « Micro » (stt) et « Jimy parle » (tts) relevés avant la
# suite, rendus dans la config à la fin même si la suite plante (veille du
# PC, 22 octobre). Jimmy est arrêté pendant l'écriture (piège 91, sans BOM).
$config = Join-Path $root 'data\config.json'
$lireInterrupteurs = "import json,sys;c=json.load(open(sys.argv[1],encoding='utf-8-sig'));print(int(c['stt']['enabled']),int(c['tts']['enabled']))"
$interrupteurs = (python -I -c $lireInterrupteurs $config).Trim()
# Canari du parcours « Sécurité » : fausse clé déposée dans le coffre avant le
# lancement (Jimmy retient les clés du coffre au démarrage), retirée à la fin.
$canariFile = Join-Path $env:USERPROFILE '.secrets\jimmy-ui-test-canari.json'
$env:JIMMY_UI_CANARI = 'canari-' + [guid]::NewGuid().ToString('N')
[System.IO.File]::WriteAllText($canariFile, "{`"api_key`": `"$($env:JIMMY_UI_CANARI)`"}")
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
Remove-Item -LiteralPath $canariFile -Force -ErrorAction SilentlyContinue
Remove-Item Env:JIMMY_UI_CANARI -ErrorAction SilentlyContinue

# Relance normale, sans port de débogage.
Stop-Jimmy
$rendreInterrupteurs = "import json,sys;p=sys.argv[1];stt,tts=(bool(int(x)) for x in sys.argv[2:4]);c=json.load(open(p,encoding='utf-8-sig'));ok=(c['stt']['enabled'],c['tts']['enabled'])==(stt,tts);c['stt']['enabled'],c['tts']['enabled']=stt,tts;ok or open(p,'w',encoding='utf-8',newline='\n').write(json.dumps(c,ensure_ascii=False,indent=2));print('interrupteurs', 'inchanges' if ok else 'restaures')"
if ($interrupteurs -match '^[01] [01]$') { python -I -c $rendreInterrupteurs $config ($interrupteurs -split ' ') }
& (Join-Path $PSScriptRoot 'launcher.ps1')
exit $code
