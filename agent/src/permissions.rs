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
            granted: false,
            allow_paths: Vec::new(),
            allow_commands: Vec::new(),
            // Motifs ancrés en début de ligne (`glob_match`) : un mot seul ne
            // bloque que la commande nue, `*` couvre ses arguments. Lot 4 :
            // destructeurs courants des trois OS, en plus des disques Windows.
            // L'extinction (`shutdown`, `reboot`, `poweroff`, `halt`) n'est
            // plus interdite en dur (v2) : elle passe par `sensitive::needs_approval`
            // (carte d'autorisation ou « confirmé » vocal, ou « Toujours autoriser »).
            deny_commands: vec![
                "format".into(),
                "diskpart".into(),
                "bcdedit".into(),
                "cipher /w".into(),
                "Remove-Item -Recurse C:\\".into(),
                "del /f /s c:\\".into(),
                "mkfs*".into(),
                "dd *".into(),
                "reg delete*".into(),
                "rm -rf /*".into(),
                "rm -rf ~*",
                "Remove-Item -Recurse C:\\Windows*".into(),
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }
}

/// Anciens motifs d'extinction de la v1, retirés de `deny_commands` en v2
/// pour laisser l'utilisateur autoriser l'arrêt du PC (carte ou permanent).
const LEGACY_POWER_DENIES: &[&str] = &["shutdown*", "reboot*", "halt*", "poweroff*"];

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
    /// Version des interdits par défaut appliqués (lot 4) : une configuration
    /// existante reçoit les nouveaux motifs sans perdre ses accords ni ses
    /// propres interdits ; un retrait volontaire postérieur est respecté.
    #[serde(default)]
    pub deny_version: u32,
}

/// Version courante des interdits par défaut (voir `merge_default_denies`).
/// v1 : destructeurs lot 4 ; v2 : retrait de `shutdown*`/`reboot*`/`halt*`/`poweroff*`
/// (désormais soumis à carte d'autorisation dans `sensitive.rs`).
pub const DENY_VERSION: u32 = 2;

impl Default for Permissions {
    fn default() -> Self {
        Permissions {
            read: AccessRule::default(),
            write: AccessRule::default(),
            execute: AccessRule::default(),
            network: AccessRule {
                granted: false,
                allow_paths: Vec::new(),
                allow_commands: Vec::new(),
                deny_commands: vec![
                    "127.0.0.1:8787*".into(),
                    "localhost:8787*".into(),
                    "[::1]:8787*".into(),
                    "127.0.0.1:8790*".into(),
                    "localhost:8790*".into(),
                    "[::1]:8790*".into(),
                ],
            },
            deny_version: DENY_VERSION,
        }
    }
}

impl Permissions {
    pub fn rule(&self, capability: Capability) -> &AccessRule {        match capability {
            Capability::Read => &self.read,
            Capability::Write => &self.write,
            Capability::Execute => &self.execute,
            Capability::Network => &self.network,
        }
    }

    /// Ajoute les interdits par défaut manquants à une configuration existante
    /// (lot 4) : les nouvelles protections profitent aux installations
    /// existantes sans toucher à leurs accords (`granted`, `allow_*`) ni
    /// retirer leurs propres interdits. Seules les règles appliquées
    /// (`execute`, `network` : `check` ignore `deny_commands` pour `read` et
    /// `write`) sont complétées. Renvoie `true` si le fichier est à réécrire.
    /// Un retrait volontaire postérieur reste respecté (version posée).
    pub fn merge_default_denies(&mut self) -> bool {
        if self.deny_version >= DENY_VERSION {
            return false;
        }
        let fresh = Permissions::default();
        for (rule, fresh_rule) in [(&mut self.execute, &fresh.execute), (&mut self.network, &fresh.network)] {
            for motif in &fresh_rule.deny_commands {
                if !rule.deny_commands.iter().any(|m| m == motif) {
                    rule.deny_commands.push(motif.clone());
                }
            }
        }
        // v2 : l'extinction passe par carte d'autorisation (`sensitive.rs`)
        // plutôt que par un refus sec dans `deny_commands`.
        if self.deny_version < 2 {
            self.execute
                .deny_commands
                .retain(|m| !LEGACY_POWER_DENIES.contains(&m.as_str()));
        }
        self.deny_version = DENY_VERSION;
        true
    }

    /// Ajoute le dossier du vault Obsidian aux chemins de **lecture** autorisés
    /// (`read.allow_paths`) s'il est restreint à une liste et ne le couvre pas
    /// encore. L'écriture directe hors inbox reste bloquée (`write.allow_paths`
    /// inchangé). Renvoie `true` si la règle a été modifiée.
    pub fn allow_vault_read(&mut self, vault_root: &std::path::Path) -> bool {
        if self.read.allow_paths.is_empty() {
            return false;
        }
        let root_str = vault_root.to_string_lossy();
        let trimmed = root_str.trim().trim_end_matches(['\\', '/']);
        if trimmed.is_empty() {
            return false;
        }
        let probe = format!("{trimmed}\\note.md");
        if self.read.allow_paths.iter().any(|p| path_match(p, &probe)) {
            return false;
        }
        self.read.allow_paths.push(format!("{trimmed}\\**"));
        true
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

    /// Vérifie seulement que la capacité est accordée, sans liste de chemins
    /// ni de commandes. Pour les magasins propres à Jimmy (mémoire locale,
    /// inbox du vault configuré) : leur emplacement est fixé par la
    /// configuration, pas par le modèle, et n'est pas un chemin à filtrer.
    pub fn check_granted(&self, capability: Capability, label: &str) -> Result<()> {
        if self.rule(capability).granted {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "la capacité « {} » n'est pas accordée ({}{})",
            capability.as_str(),
            capability.as_str(),
            suffix(label)
        )))
    }

    /// Accès « standard » : ceux qu'avait toute installation avant le refus
    /// par défaut (10 octobre) — lecture, écriture et exécution dans le
    /// dossier utilisateur et `C:\Dev`, réseau ouvert — en gardant les
    /// interdits (commandes destructrices, ports locaux de l'avatar et du
    /// pont). Sert à la migration des installations existantes et aux cases
    /// cochées de l'écran d'accueil.
    pub fn standard() -> Self {
        let refused = Permissions::default();
        let open = |rule: AccessRule| AccessRule {
            granted: true,
            allow_paths: vec!["C:\\Users\\**".into(), "C:\\Dev\\**".into()],
            allow_commands: vec!["*".into()],
            ..rule
        };
        Permissions {
            read: open(refused.read),
            write: open(refused.write),
            execute: open(refused.execute),
            network: AccessRule { granted: true, ..refused.network },
            deny_version: DENY_VERSION,
        }
    }

    /// Accès choisis à l'écran d'accueil : chaque capacité cochée reçoit les
    /// accès standard, les autres restent refusées.
    pub fn from_grants(read: bool, write: bool, execute: bool, network: bool) -> Self {
        let standard = Permissions::standard();
        let refused = Permissions::default();
        Permissions {
            read: if read { standard.read } else { refused.read },
            write: if write { standard.write } else { refused.write },
            execute: if execute { standard.execute } else { refused.execute },
            network: if network { standard.network } else { refused.network },
            deny_version: DENY_VERSION,
        }
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

/// Crée `permissions.json` s'il manque (piège 104). Sans lui, les défauts
/// s'appliquent, et depuis le refus par défaut une installation existante
/// se retrouvait sans aucun accès. Installation existante (accueil déjà
/// terminé) : accès standard, comme avant. Nouvelle installation : tout
/// refusé, l'écran d'accueil demande les accords. Un fichier présent n'est
/// jamais touché. Renvoie `true` si le fichier a été créé.
pub fn ensure_file(path: &std::path::Path, existing_install: bool) -> bool {
    if path.is_file() {
        return false;
    }
    let (permissions, why) = if existing_install {
        (Permissions::standard(), "installation existante : accès standard conservés")
    } else {
        (Permissions::default(), "nouvelle installation : tout refusé jusqu'à l'accueil")
    };
    let written = serde_json::to_string_pretty(&permissions)
        .map_err(|e| e.to_string())
        .and_then(|raw| std::fs::write(path, raw).map_err(|e| e.to_string()));
    match written {
        Ok(()) => {
            log::info!("[permissions] {} créé — {why}", path.display());
            true
        }
        Err(e) => {
            log::warn!("[permissions] {} non créé : {e}", path.display());
            false
        }
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

    /// Piège 104 : une installation existante sans `permissions.json` garde
    /// ses accès ; une nouvelle reste tout refusé ; un fichier présent n'est
    /// jamais écrasé.
    #[test]
    fn migration_du_fichier_de_permissions() {
        let dir = std::env::temp_dir().join(format!("perms-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let existing = dir.join("existante.json");
        assert!(ensure_file(&existing, true));
        let raw = std::fs::read_to_string(&existing).expect("lu");
        let loaded: Permissions = serde_json::from_str(&raw).expect("json");
        assert!(loaded.check(Capability::Write, r"C:\Users\user\Projet\a.txt").is_ok());
        assert!(loaded.check(Capability::Execute, "git status").is_ok());
        assert!(loaded.check(Capability::Execute, "diskpart").is_err());
        assert!(loaded.check(Capability::Network, "127.0.0.1:8787").is_err());

        let fresh = dir.join("neuve.json");
        assert!(ensure_file(&fresh, false));
        let raw = std::fs::read_to_string(&fresh).expect("lu");
        let loaded: Permissions = serde_json::from_str(&raw).expect("json");
        assert!(loaded.check(Capability::Read, r"C:\Users\user\notes.txt").is_err());

        // Le choix de l'utilisateur n'est jamais écrasé.
        std::fs::write(&fresh, r#"{"choix":"utilisateur"}"#).expect("écriture");
        assert!(!ensure_file(&fresh, true));
        assert_eq!(std::fs::read_to_string(&fresh).expect("lu"), r#"{"choix":"utilisateur"}"#);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn accueil_accorde_seulement_les_cases_cochees() {
        let perms = Permissions::from_grants(true, false, false, false);
        assert!(perms.check(Capability::Read, r"C:\Users\user\notes.txt").is_ok());
        assert!(perms.check(Capability::Write, r"C:\Users\user\notes.txt").is_err());
        assert!(perms.check(Capability::Execute, "git status").is_err());
        assert!(!perms.network.granted);
    }

    #[test]
    fn lecture_bloquee_refuse() {
        let mut perms = Permissions::default();
        perms.read.granted = true;
        perms.read.granted = false;
        assert!(perms.check(Capability::Read, "C:\\Users\\user\\notes.txt").is_err());
    }

    #[test]
    fn chemins_autorises_respectes() {
        let mut perms = Permissions::default();
        perms.read.granted = true;
        perms.read.allow_paths = vec!["C:\\Users\\**".into()];
        assert!(perms.check(Capability::Read, r"C:\Users\user\Projet\a.rs").is_ok());
        assert!(perms.check(Capability::Read, r"C:\Windows\System32\config").is_err());
    }

    #[test]
    fn commandes_dangereuses_bloquees() {
        let mut perms = Permissions::default();
        perms.execute.granted = true;
        perms.execute.allow_commands = vec!["git *".into()];
        assert!(perms.check(Capability::Execute, "git status").is_ok());
        assert!(perms.check(Capability::Execute, "diskpart").is_err());
    }

    /// Lot 4 + v2 : les destructeurs irréversibles des trois OS sont interdits
    /// par défaut (`rm -rf /`, `mkfs`, `reg delete`…), tandis que l'extinction
    /// (`shutdown`, `reboot`) passe par carte d'autorisation (`sensitive`).
    #[test]
    fn destructeurs_multi_os_bloques_ordinaires_libres() {
        let mut perms = Permissions::default();
        perms.execute.granted = true;
        for advocate in ["mkfs /dev/sda", "dd if=/dev/zero of=/dev/sda", "reg delete HKLM /f", "rm -rf /", "rm -rf ~/docs", "Remove-Item -Recurse C:\\Windows\\Temp"] {
            assert!(perms.check(Capability::Execute, advocate).is_err(), "{advocate} doit être interdit");
        }
        for ordinary in ["git status", "npm run build", "Get-ChildItem C:\\Projet", "echo halte", "shutdown /s /t 15"] {
            assert!(perms.check(Capability::Execute, ordinary).is_ok(), "{ordinary} doit rester libre côté deny_commands");
        }
    }

    /// Les nouveaux interdits rejoignent une configuration existante sans
    /// toucher à ses accords ; la v2 retire l'ancien `shutdown*` rigide.
    #[test]
    fn les_nouveaux_interdits_completent_sans_ecraser() {
        let mut perms = Permissions::standard();
        perms.deny_version = 1;
        perms.execute.deny_commands.push("shutdown*".into());
        perms.execute.deny_commands.retain(|m| m != "mkfs*");
        assert!(perms.merge_default_denies());
        assert!(perms.execute.deny_commands.iter().any(|m| m == "mkfs*"));
        assert!(!perms.execute.deny_commands.iter().any(|m| m == "shutdown*"), "shutdown* retiré en v2");
        assert!(perms.execute.granted, "les accords ne bougent pas");
        assert!(perms.check(Capability::Execute, "git status").is_ok());
        assert!(!perms.merge_default_denies(), "deuxième passage : rien à faire");
    }

    /// Le dossier du vault configuré rejoint `read.allow_paths` sans ouvrir
    /// l'écriture directe hors inbox.
    #[test]
    fn vault_ajoute_en_lecture_seulement() {
        let mut perms = Permissions::standard();
        assert!(perms.check(Capability::Read, r"C:\Obsidian\Jimmy\1_Projets\Preuve_Lot\cibles.csv").is_err());
        assert!(perms.allow_vault_read(std::path::Path::new(r"C:\Obsidian\Jimmy")));
        assert!(perms.check(Capability::Read, r"C:\Obsidian\Jimmy\1_Projets\Preuve_Lot\cibles.csv").is_ok());
        assert!(perms.check(Capability::Write, r"C:\Obsidian\Jimmy\1_Projets\Preuve_Lot\cibles.csv").is_err());
        assert!(!perms.allow_vault_read(std::path::Path::new(r"C:\Obsidian\Jimmy")), "idempotent");
    }
}