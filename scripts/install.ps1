# Installateur Jimmy
#
# Vérifie les prérequis, installe ce qui manque, compile l'application.
#
#   .\scripts\install.ps1                    # tout
#   .\scripts\install.ps1 -SkipBuildTools    # sans Visual Studio (déjà là)
#   .\scripts\install.ps1 -SkipModels        # sans télécharger les modèles vocaux
#   .\scripts\install.ps1 -NoBuild           # vérifications seules
#
# Sortie : 0 si tout est prêt, 1 s'il reste des prérequis à traiter.

[CmdletBinding()]
param(
    [switch]$SkipBuildTools,
    [switch]$SkipModels,
    [switch]$SkipGodot,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

$root = Split-Path -Parent $PSScriptRoot
$whisperDir = Join-Path $root 'data\components\whisper'
$modelsDir = Join-Path $whisperDir 'models'
$missing = New-Object System.Collections.ArrayList
$done = New-Object System.Collections.ArrayList

function Say-Ok([string]$m) { Write-Host "  [ok]     $m" }
function Say-Skip([string]$m) { Write-Host "  [ignore] $m" }
function Say-Work([string]$m) { Write-Host "  ...      $m" }
function Say-Missing([string]$m) {
    Write-Host "  [MANQUE] $m" -ForegroundColor Red
    [void]$missing.Add($m)
}
function Say-Step([string]$m) { Write-Host "`n$m" -ForegroundColor Cyan }

# ── 1. Rust et Node ──────────────────────────────────────────────────────────

Say-Step '1. Outils de compilation'

# rustup vit par défaut hors du PATH de la session Windows ; on l'ajoute.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Get-Command rustc -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $cargoBin 'rustc.exe'))) {
    $env:PATH = "$cargoBin;$env:PATH"
}

if (Get-Command rustc -ErrorAction SilentlyContinue) {
    Say-Ok ("rustc " + (& rustc --version))
} else {
    Say-Missing 'Rust absent — installez rustup : https://rustup.rs'
}

if (Get-Command node -ErrorAction SilentlyContinue) {
    Say-Ok ("node " + (& node --version))
} else {
    Say-Missing 'Node.js absent — https://nodejs.org'
}

# ── 2. Visual Studio Build Tools ────────────────────────────────────────────

Say-Step '2. Visual Studio Build Tools (C++)'

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$hasCxx = $false
if (Test-Path $vswhere) {
    $found = & $vswhere -products * -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($found) {
        $hasCxx = $true
        Say-Ok "VC.Tools : $found"
    }
}

if (-not $hasCxx -and $SkipBuildTools) {
    Say-Skip 'composant C++ absent — la compilation échouera'
}
elseif (-not $hasCxx) {
    Say-Work 'installation de Visual Studio Build Tools (demande les droits administrateur)…'
    $wingetArgs = 'install --id Microsoft.VisualStudio.2022.BuildTools --source winget'
    $wingetArgs += ' --accept-package-agreements --accept-source-agreements --disable-interactivity'
    $wingetArgs += ' --override "--quiet --wait --norestart --nocache'
    $wingetArgs += ' --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
    Invoke-Expression "winget $wingetArgs" | Out-Host
    if (Test-Path $vswhere) {
        $found = & $vswhere -products * -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($found) {
            $hasCxx = $true
            [void]$done.Add('Visual Studio Build Tools')
            Say-Ok "installé : $found"
        }
    }
}

if (-not $hasCxx) {
    Say-Missing 'composant C++ (VC.Tools.x86.x64) absent'
}

# ── 3. WebView2 ─────────────────────────────────────────────────────────────

Say-Step '3. WebView2 (moteur de la fenêtre)'

$wvRoot = Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'
$wv = $null
if (Test-Path $wvRoot) {
    foreach ($d in (Get-ChildItem $wvRoot -Directory -ErrorAction SilentlyContinue)) {
        if ($d.Name -match '^[0-9]+\.[0-9]+') { $wv = $d }
    }
}
if ($wv) { Say-Ok "WebView2 $($wv.Name)" }
else { Say-Skip 'WebView2 introuvable — présent d''office sur Windows 11 ; sinon https://developer.microsoft.com/microsoft-edge/webview2/' }

# ── 4. Godot ────────────────────────────────────────────────────────────────

Say-Step '4. Godot (avatar 3D)'

$godotExe = $null
if (Test-Path 'C:\Dev\Godot') {
    foreach ($f in (Get-ChildItem 'C:\Dev\Godot' -Recurse -Filter 'Godot*_win64.exe' -File -ErrorAction SilentlyContinue)) {
        if ($f.Name -notmatch 'mono') { $godotExe = $f.FullName; break }
    }
}
if (-not $godotExe -and (Test-Path 'C:\Program Files\Godot')) {
    foreach ($f in (Get-ChildItem 'C:\Program Files\Godot' -Recurse -Filter 'Godot*_win64.exe' -File -ErrorAction SilentlyContinue)) {
        if ($f.Name -notmatch 'mono') { $godotExe = $f.FullName; break }
    }
}

if ($godotExe) { Say-Ok $godotExe }
elseif ($SkipGodot) { Say-Skip 'Godot absent — ignoré' }
else { Say-Missing 'Godot absent — https://godotengine.org/download (4.5 ou plus récent, build standard)' }

# ── 5. Reconnaissance vocale locale ─────────────────────────────────────────

Say-Step '5. Reconnaissance vocale locale (whisper.cpp)'

$server = Join-Path $whisperDir 'Release\whisper-server.exe'
if (Test-Path $server) {
    Say-Ok 'whisper-server déjà présent'
}
elseif ($SkipModels) {
    Say-Skip 'whisper-server absent — ignoré'
}
else {
    Say-Work 'téléchargement de whisper.cpp (~8 Mo)…'
    $zipPath = Join-Path $env:TEMP 'jimmy-whisper.zip'
    try {
        Invoke-WebRequest 'https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip' -OutFile $zipPath -UseBasicParsing -TimeoutSec 600
        Expand-Archive -Path $zipPath -DestinationPath $whisperDir -Force
        Remove-Item $zipPath -Force
        if (Test-Path $server) {
            [void]$done.Add('whisper.cpp')
            Say-Ok 'whisper-server installé'
        }
        else { Say-Missing 'whisper-server introuvable après extraction' }
    }
    catch {
        Say-Missing ("whisper.cpp : " + $_.Exception.Message)
    }
}

$models = @(
    @{ n = 'ggml-base-q5_1.bin'; u = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin'; req = $true },
    @{ n = 'ggml-silero-v6.2.0.bin'; u = 'https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin'; req = $true },
    @{ n = 'ggml-small-q5_1.bin'; u = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin'; req = $false }
)
foreach ($m in $models) {
    $target = Join-Path $modelsDir $m.n
    if (Test-Path $target) { Say-Ok "$($m.n) déjà présent" }
    elseif ($SkipModels) { Say-Skip "$($m.n) absent — ignoré" }
    else {
        Say-Work "téléchargement de $($m.n)…"
        try {
            New-Item -ItemType Directory -Force -Path $modelsDir | Out-Null
            Invoke-WebRequest $m.u -OutFile $target -UseBasicParsing -TimeoutSec 1800
            $mb = [math]::Round((Get-Item $target).Length / 1MB, 1)
            Say-Ok "$($m.n) ($mb Mo)"
        }
        catch {
            if ($m.req) { Say-Missing ("$($m.n) : " + $_.Exception.Message) }
            else { Say-Skip "$($m.n) : téléchargement échoué (facultatif)" }
        }
    }
}

# ── 6. Dépendances de l'interface ───────────────────────────────────────────

Say-Step '6. Dépendances de l''interface'

Push-Location (Join-Path $root 'desktop')
if (Test-Path 'node_modules') {
    Say-Ok 'node_modules présent'
}
else {
    Say-Work 'npm install…'
    npm install --no-audit --no-fund | Out-Host
    if (Test-Path 'node_modules') { Say-Ok 'dépendances installées' }
    else { Say-Missing 'npm install a échoué' }
}
Pop-Location

# ── 7. Configuration ────────────────────────────────────────────────────────

Say-Step '7. Configuration'

$envFile = Join-Path $root '.env'
if (Test-Path $envFile) {
    Say-Ok '.env présent'
}
else {
    Copy-Item (Join-Path $root '.env.example') $envFile
    Say-Ok '.env créé — renseignez vos clés API'
    [void]$missing.Add('clés API à renseigner dans .env')
}

# ── 8. Compilation ──────────────────────────────────────────────────────────

if (-not $NoBuild) {
    Say-Step '8. Compilation'
    & (Join-Path $root 'scripts\build.ps1') | Out-Host
    if ($LASTEXITCODE -ne 0) { [void]$missing.Add('compilation') }
    else { Say-Ok 'compilation réussie' }
}

# ── Bilan ───────────────────────────────────────────────────────────────────

Write-Host ''
Write-Host 'Bilan' -ForegroundColor Cyan
foreach ($d in $done) { Write-Host "  installé : $d" }
if ($missing.Count -gt 0) {
    Write-Host "  à traiter ($($missing.Count)) :" -ForegroundColor Yellow
    foreach ($p in $missing) { Write-Host "    - $p" -ForegroundColor Yellow }
    exit 1
}
Write-Host '  Tout est en place. Lancez : .\scripts\dev.ps1' -ForegroundColor Green
exit 0