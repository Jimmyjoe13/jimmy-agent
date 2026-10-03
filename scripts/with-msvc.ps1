param(
    # Commande à exécuter avec l'environnement MSVC (vcvars64) chargé.
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)

# Jimmy — lance une commande dans l'environnement de développement Rust/MSVC.
#
# Rust Targete `x86_64-pc-windows-msvc` : le linker (link.exe) et les
# bibliothèques C runtime sont fournis par les Visual Studio Build Tools, qui
# vivent dans leur propre environnement. Ce script charge `vcvars64.bat` puis
# exécute la commande demandée, afin qu'aucune session n'ait à configurer quoi
# que ce soit manuellement.
#
# Exemples :
#   .\scripts\with-msvc.ps1 cargo build
#   .\scripts\with-msvc.ps1 cargo test --workspace
#   .\scripts\with-msvc.ps1 cmd /c "cargo tauri dev"

$ErrorActionPreference = 'Stop'

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    throw "Visual Studio Build Tools introuvable. Installez-les avec : winget install Microsoft.VisualStudio.2022.BuildTools --override `"--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`""
}

$installPath = (& $vswhere -products * -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1).Trim()
if (-not $installPath) {
    throw "Composant C++ (VC.Tools.x86.x64) introuvable dans Visual Studio Build Tools."
}

$vcvars = Join-Path $installPath 'VC\Auxiliary\Build\vcvars64.bat'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Test-Path $vcvars)) { throw "vcvars64.bat introuvable dans $installPath" }

# On construit une ligne de commande unique : `vcvars64` n'expose que des
# variables d'environnement, il doit donc s'exécuter dans le même shell.
$cmdLine = "`"$vcvars`" >nul 2>nul && set `"PATH=$cargoBin;%PATH%`" && " + ($Command -join ' ')
& cmd.exe /d /s /c $cmdLine
exit $LASTEXITCODE