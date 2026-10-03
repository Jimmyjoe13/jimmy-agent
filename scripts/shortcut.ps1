# Crée le raccourci « Jimmy » sur le Bureau.
#
#   .\scripts\shortcut.ps1            # crée ou met à jour le raccourci
#   .\scripts\shortcut.ps1 -Remove    # supprime le raccourci
#   .\scripts\shortcut.ps1 -Autostart # ajoute aussi une entrée « Démarrage »
#
# Le raccourci pointe sur scripts\launcher.ps1, masqué : ni fenêtre de console,
# ni compilation au lancement. C'est ce qui permet de double-cliquer sur le
# bureau et d'oublier le projet.

[CmdletBinding()]
param(
    [switch]$Remove,
    [switch]$Autostart
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$launcher = Join-Path $root 'scripts\launcher.ps1'
$desktop = [Environment]::GetFolderPath('Desktop')
$startup = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup'
$icon = Join-Path $root 'desktop\src-tauri\icons\128x128.png'

if (-not (Test-Path $launcher)) {
    throw "Lanceur introuvable : $launcher"
}

function New-JimmyRaccourci([string]$dossier, [string]$nom) {
    $chemin = Join-Path $dossier "$nom.lnk"
    $shell = New-Object -ComObject WScript.Shell
    $raccourci = $shell.CreateShortcut($chemin)
    $raccourci.TargetPath = 'powershell.exe'
    # -WindowStyle Hidden : pas de fenêtre au clic. -ExecutionPolicy Bypass :
    # le lanceur n'a pas besoin d'être dans la politique d'exécution.
    $raccourci.Arguments = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$launcher`""
    $raccourci.WorkingDirectory = $root
    $raccourci.Description = 'Jimmy — assistant IA personnel sur le bureau'
    if (Test-Path $icon) {
        $raccourci.IconLocation = "$icon,0"
    }
    $raccourci.Save()
    return $chemin
}

if ($Remove) {
    $lnk = Join-Path $desktop 'Jimmy.lnk'
    if (Test-Path $lnk) {
        Remove-Item $lnk -Force
        Write-Host "Raccourci supprimé : $lnk" -ForegroundColor Yellow
    } else {
        Write-Host 'Aucun raccourci à supprimer.'
    }
    exit 0
}

$cree = New-JimmyRaccourci $desktop 'Jimmy'
Write-Host "Raccourci créé : $cree" -ForegroundColor Green

if ($Autostart) {
    if (-not (Test-Path $startup)) {
        New-Item -ItemType Directory -Force -Path $startup | Out-Null
    }
    $lnk = New-JimmyRaccourci $startup 'Jimmy'
    Write-Host "Démarrage avec Windows activé : $lnk" -ForegroundColor Green
} else {
    Write-Host "Pour démarrer avec Windows : .\scripts\shortcut.ps1 -Autostart"
}

Write-Host ''
Write-Host 'Un double-clic sur « Jimmy » lance l''application.' -ForegroundColor Cyan