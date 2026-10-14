//! Socle système : ce que Jimmy sait de la machine qu'il pilote.
//!
//! Un assistant de bureau parle au terminal natif : PowerShell sur Windows,
//! `zsh` (sinon `sh`) ailleurs. Avant, `run_command` appelait
//! `powershell.exe` en dur — inutilisable sur macOS (chantier installateur
//! macOS, lot 4). Les outils natifs lisent ici l'OS au lieu de le deviner.

/// Nom du système : `windows`, `macos`, `linux`, …
pub fn name() -> &'static str {
    std::env::consts::OS
}

/// Vrai sur macOS (chemins `~/Library`, `osascript`, `launchctl`).
pub fn is_macos() -> bool {
    std::env::consts::OS == "macos"
}

/// Interpréteur natif : programme + arguments fixes (aucune entrée du modèle
/// ne transite par ici, donc aucune injection possible à ce niveau).
pub fn shell() -> (&'static str, &'static [&'static str]) {
    if std::env::consts::OS == "windows" {
        ("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command"])
    } else if is_macos() {
        ("/bin/zsh", &["-lc"])
    } else {
        ("/bin/sh", &["-lc"])
    }
}

/// Coquille d'une fiche machine lisible par le modèle (sans secret, sans
/// chemin personnel) : OS, architecture, interpréteur, cœurs.
pub fn describe() -> String {
    let (program, _) = shell();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    format!(
        "système : {} ({}) ; interpréteur : {program} ; cœurs : {cores}",
        name(),
        std::env::consts::ARCH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le dispatcher suit l'OS de compilation : PowerShell sur Windows, un
    /// shell POSIX ailleurs. Arguments fixes, jamais du modèle.
    #[test]
    fn l_interpreteur_suit_le_systeme() {
        let (program, args) = shell();
        assert!(!program.is_empty());
        if std::env::consts::OS == "windows" {
            assert_eq!(program, "powershell.exe");
            assert!(args.contains(&"-Command"));
        } else {
            assert!(args.contains(&"-lc"), "{program}");
        }
        let fiche = describe();
        assert!(fiche.contains(name()) && fiche.contains(std::env::consts::ARCH), "{fiche}");
    }
}
