# Construit l'installeur Windows complet de Jimy (NSIS).
#
#   .\scripts\build-installer.ps1
#
# Contrairement à `build.ps1 -Release -Bundles`, l'installeur embarque
# l'exécutable Godot 4.5.1 (licence MIT, redistribuable), pris sur cette
# machine s'il est trouvé, sinon téléchargé depuis les releases Godot. Le
# projet de l'avatar (`godot/`) est, lui, intégré au binaire de Jimy
# (agent/build.rs) : aucun script modifiable n'est livré en clair.
# whisper.cpp et ses modèles ne sont pas embarqués : Jimy les télécharge au
# premier lancement (agent/src/components.rs).
#
# Les ressources sont préparées dans `desktop/src-tauri/installer-res/`
# (ignoré par git) et déclarées dans `tauri.installer.conf.json`, fusionnée
# seulement ici : la compilation de tous les jours n'est pas alourdie.
#
# Sortie : chemin de l'installeur (target\release\bundle\nsis\*.exe).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$res = Join-Path $root 'desktop\src-tauri\installer-res'
$godotName = 'Godot_v4.5.1-stable_win64.exe'

if (Test-Path $res) { Remove-Item -Recurse -Force $res }
New-Item -ItemType Directory -Force -Path (Join-Path $res 'godot-runtime') | Out-Null

Write-Host '1. Exécutable Godot' -ForegroundColor Cyan
# @(...) autour du filtre : un seul résultat resterait sinon une chaîne, et
# `$candidates[0]` en donnerait la première lettre.
$candidates = @(@(
    $env:JIMMY_GODOT_EXE,
    "C:\Dev\Godot\$godotName\$godotName",
    "C:\Dev\Godot\$godotName",
    "C:\Program Files\Godot\$godotName",
    (Join-Path $env:USERPROFILE "Godot\$godotName")
) | Where-Object { $_ -and (Test-Path $_ -PathType Leaf) })
$target = Join-Path $res "godot-runtime\$godotName"
if ($candidates) {
    Copy-Item $candidates[0] $target
    Write-Host "   copié depuis la machine"
}
else {
    $zip = Join-Path $env:TEMP 'godot-4.5.1.zip'
    Invoke-WebRequest "https://github.com/godotengine/godot/releases/download/4.5.1-stable/$godotName.zip" -OutFile $zip -UseBasicParsing -TimeoutSec 1800
    Expand-Archive -Path $zip -DestinationPath (Join-Path $res 'godot-runtime') -Force
    Remove-Item $zip -Force
    Write-Host "   téléchargé depuis les releases Godot"
}
if (-not (Test-Path $target)) { throw "Godot introuvable dans les ressources" }

Write-Host '2. Installeur NSIS' -ForegroundColor Cyan
Push-Location (Join-Path $root 'desktop')
& (Join-Path $root 'scripts\with-msvc.ps1') npx tauri build --bundles nsis --config src-tauri/tauri.installer.conf.json
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { exit $code }

$setup = Get-ChildItem (Join-Path $root 'target\release\bundle\nsis') -Filter '*-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Host ("Installeur : {0} ({1:N0} Mo)" -f $setup.FullName, ($setup.Length / 1MB)) -ForegroundColor Green
