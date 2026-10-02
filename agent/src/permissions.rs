//! Système de permissions.
//!
//! Trois capacités, conformément au PLAN : **lecture**, **modification**,
//! **exécution**. Une fois accordée, l'action correspondante s'exécute sans
//! nouvelle confirmation ; c'est le comportement demandé pour la V1 personnelle.
//!
//! La structure est déjà prête pour un raffinement futur (permissions par
//! outil, par service, par application, par dossier, par capacité) : une règle
//! porte une liste de chemins autorisés et une liste de commandes autorisées,
//! chacune évaluée indépendamment.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Lire un fichier ou lister un dossier.
    Read,
    /// Écrire, créer, déplacer ou supprimer.
    Write,
    /// Lancer un processus ou ouvrir une application.
    Execute,
    /// Accéder au réseau hors fournisseurs déjà configurés.
    Network,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Write => "write",
            Capability::Execute => "execute",
            Capability::Network => "network",
        }
    }
}

/// Une règle d'accès. `granted = false` interdit la capacité.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRule {
    /// `true` : accordé, Jimmy agit sans redemander.
    pub granted: bool,
    /// Chemins autorisés (motifs `*`). Vide = tous les chemins.
    #[serde(default)]
    pub allow_paths: Vec<String>,
    /// Motifs de commandes autorisées (`git *`). Vide = toutes.
    #[serde(default)]
    pub allow_commands: Vec<String>,
    /// Motifs explicitement refusés, évalués en priorité.
    #[serde(default)]
    pub deny_commands: Vec<String>,
}

impl Default for AccessRule {
    fn default() -> Self {
        AccessRule {
            granted: true,
            allow_paths: vec!["C:\\Users\\**".into(), "C:\\Dev\\**".into()],
            allow_commands: vec!["*".into()],
            deny_commands: vec![
                "format".into(),
                "diskpart".into(),
                "bcdedit".into(),
                "cipher /w".into(),
                "Remove-Item -Recurse C:\\".into(),
                "del /f /s c:\\".into(),
            ],
        }
    }
}

impl AccessRule {
    pub fn deny_all() -> Self {
        AccessRule {
            granted: false,
            allow_paths: Vec::new(),
            allow_commands: Vec::new(),
            deny_commands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    pub read: AccessRule,
    pub write: AccessRule,
    pub execute: AccessRule,
    pub network: AccessRule,
}

impl Default for Permissions {
    fn default() -> Self {
        Permissions {
            read: AccessRule::default(),
            write: AccessRule::default(),
            execute: AccessRule::default(),
            network: AccessRule {
                granted: true,
                allow_paths: Vec::new(),
                allow_commands: Vec::new(),
                deny_commands: Vec::new(),
            },
        }
    }
}

impl Permissions {
    pub fn rule(&self, capability: Capability) -> &AccessRule {
        match capability {
            Capability::Read => &self.read,
            Capability::Write => &self.write,
            Capability::Execute => &self.execute,
            Capability::Network => &self.network,
        }
    }

    /// Vérifie qu'une opération est autorisée. `target` est un chemin (Read,
    /// Write) ou une ligne de commande (Execute). Réseau : `target` est l'hôte.
    pub fn check(&self, capability: Capability, target: &str) -> Result<()> {
        let rule = self.rule(capability);
        if !rule.granted {
            return Err(Error::PermissionDenied(format!(
                "la capacité « {} » n'est pas accordée ({}{})",
                capability.as_str(),
                capability.as_str(),
                suffix(target)
            )));
        }
        match capability {
            Capability::Execute | Capability::Network => {
                if let Some(denied) = rule
                    .deny_commands
                    .iter()
                    .find(|pattern| glob_match(pattern, target))
                {
                    return Err(Error::PermissionDenied(format!(
                        "la commande est sur la liste interdite ({denied})"
                    )));
                }
                if !rule.allow_commands.is_empty()
                    && !rule.allow_commands.iter().any(|p| glob_match(p, target))
                {
                    return Err(Error::PermissionDenied(format!(
                        "commande non autorisée : {target}"
                    )));
                }
            }
            Capability::Read | Capability::Write => {
                if !rule.allow_paths.is_empty()
                    && !rule
                        .allow_paths
                        .iter()
                        .any(|p| path_match(p, target))
                {
                    return Err(Error::PermissionDenied(format!(
                        "chemin non autorisé : {target}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Résumé lisible pour l'interface.
    pub fn summary(&self) -> Vec<(Capability, bool)> {
        vec![
            (Capability::Read, self.read.granted),
            (Capability::Write, self.write.granted),
            (Capability::Execute, self.execute.granted),
            (Capability::Network, self.network.granted),
        ]
    }
}

fn suffix(target: &str) -> String {
    if target.is_empty() {
        String::new()
    } else {
        format!(" — {target}")
    }
}

/// Correspondance de motif simple : `*` = n'importe quoi, `?` = un caractère.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    // Une commande peut commencer par des espaces.
    let t: Vec<char> = text.trim_start().to_lowercase().chars().collect();
    // Algorithme de glissement : deux index suffisent pour un motif libre.
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, usize::MAX, 0usize);
    while i < t.len() {
        if j < p.len() && (p[j] == '?' || p[j] == t[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = j;
            mark = i;
            j += 1;
        } else if star != usize::MAX {
            j = star + 1;
            mark += 1;
            i = mark;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == '*' {
        j += 1;
    }
    j == p.len()
}

/// Correspondance de chemin : insensible à la casse, `**` traverse les
/// dossiers, `*` s'arrête au séparateur.
pub fn path_match(pattern: &str, target: &str) -> bool {
    let p = pattern.to_lowercase().replace('/', "\\");
    let t = target.to_lowercase().replace('/', "\\");
    if p == "*" || p == "**" || p == "c:\\" {
        return true;
    }
    if p.ends_with("\\**") {
        let prefix = &p[..p.len() - 3];
        return t == prefix || t.starts_with(&format!("{prefix}\\"));
    }
    p == t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_bloquee_refuse() {
        let mut perms = Permissions::default();
        perms.read.granted = false;
        assert!(perms.check(Capability::Read, "C:\\Users\\jimmy\\notes.txt").is_err());
    }

    #[test]
    fn chemins_autorises_respectes() {
        let perms = Permissions::default();
        assert!(perms.check(Capability::Read, r"C:\Users\jimmy\Projet\a.rs").is_ok());
        assert!(perms.check(Capability::Read, r"C:\Windows\System32\config").is_err());
    }

    #[test]
    fn commandes_dangereuses_bloquees() {
        let perms = Permissions::default();
        assert!(perms.check(Capability::Execute, "git status").is_ok());
        assert!(perms.check(Capability::Execute, "diskpart").is_err());
    }
}