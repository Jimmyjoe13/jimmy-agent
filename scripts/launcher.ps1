# Lance Jimmy directement, sans terminal ni compilation.
#
# Ce script est la cible du raccourci Bureau. Il a une seule responsabilité :
# démarrer le binaire et, s'il est absent, le dire clairement plutôt que
# d'échouer en silence.
#
# Deux modes, choisis automatiquement :
#   • binaire release présent  -> exécution directe, aucun terminal visible ;
#   • sinon                    -> bascule sur le mode développement
#                                 (scripts\dev.ps1), qui compile au lancement.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$release = Join-Path $root 'target\release\jimmy.exe'

function Say-Erreur([string]$m) {
    Add-Type -AssemblyName System.Windows.Forms | Out-Null
    [System.Windows.Forms.MessageBox]::Show(
        $m,
        'Jimmy',
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
}

# Le dossier racine en répertoire courant : c'est lui que l'application utilise
# pour retrouver le projet Godot, les skills et les données.
Set-Location $root

if (Test-Path $release) {
    Start-Process -FilePath $release -WorkingDirectory $root
    exit 0
}

# Pas de binaire release. Le binaire de debug n'est PAS un repli possible : en
# développement il attend un serveur Vite, et double-cliquer sur le raccourci ne
# le lance pas. On explique donc, et on propose de compiler.

# Aucun binaire : on explique quoi faire plutôt que d'ouvrir une console qui
# disparaît aussitôt.
Say-Erreur @"
Jimmy n'est pas encore compilé.

Ouvre une invite PowerShell dans le dossier du projet et lance :

    .\scripts\build.ps1 -Release

La première compilation prend quelques minutes.
"@

# Par commodité, on propose de le lancer tout de suite.
$racine = $root
if ((New-Object System.Windows.Forms.MessageBox).Show(
        "Voulez-vous lancer la compilation maintenant ?",
        'Jimmy',
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        [System.Windows.Forms.MessageBoxIcon]::Question) -eq 'Yes') {
    Start-Process -FilePath 'powershell.exe' `
        -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $root 'scripts\build.ps1') `
        -WorkingDirectory $racine
}
exit 1