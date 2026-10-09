//! Mémoire persistante au format Obsidian : le vault de l'utilisateur.
//!
//! Jimmy n'a plus besoin d'un service de mémoire externe : ses souvenirs sont
//! des notes Markdown dans le vault, relues par l'utilisateur comme les autres
//! notes, et recherchées en plein texte à la volée (le vault fait quelques
//! mégaoctets : tout parcourir à chaque requête reste instantané).
//!
//! Sens de lecture : Jimmy **écrit** dans son dossier du vault
//! (`vault_folder`, par défaut `0_Inbox/Jimmy`) et **lit** partout dans le
//! vault — ce qui lui donne accès aux projets, ressources et archives de
//! l'utilisateur, pas seulement à ce qu'il a lui-même appris.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Taille maximale d'une note lue en entier (une note plus grande est un
/// journal ou un export, pas un souvenir) : 300 Ko.
const MAX_NOTE_BYTES: u64 = 300 * 1024;

/// Limite d'une sortie d'outil, en caractères.
const MAX_OUTPUT_CHARS: usize = 12_000;

#[derive(Debug, Clone)]
pub struct Vault {
    /// Racine du vault, ex. `C:\Obsidian\Jimmy`.
    root: PathBuf,
    /// Sous-dossier où Jimmy écrit ses souvenirs, ex. `0_Inbox/Jimmy`.
    folder: PathBuf,
    /// D'où vient ce vault (chemin réglé, registre d'Obsidian, vault propre).
    origin: VaultOrigin,
    /// Sérialise les écritures dans l'inbox : captures concurrentes (tâches
    /// de fond) et mise de côté des fichiers par le tri (`memory::sort`).
    pub(crate) inbox_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

/// Archive des captures déjà triées (relisibles, mais hors recherche : leur
/// contenu est déjà dans les notes `Jimy_…`).
pub const INBOX_ARCHIVE: &str = "4_Archives/Jimy/Inbox";

/// Provenance du vault ouvert, affichée dans l'interface et le journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultOrigin {
    /// Chemin choisi dans les Paramètres (ou `JIMMY_VAULT_PATH`).
    Configured,
    /// Trouvé dans le registre d'Obsidian de la machine.
    Obsidian,
    /// Aucun vault sur la machine : vault propre créé dans le dossier de Jimy.
    Own,
}

impl VaultOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            VaultOrigin::Configured => "configured",
            VaultOrigin::Obsidian => "obsidian",
            VaultOrigin::Own => "own",
        }
    }
}

/// Architecture PARA, la même que le vault de l'utilisateur : capture dans
/// l'Inbox, puis Projets, Casquettes (responsabilités continues), Ressources,
/// Archives ; `_SYSTEM` pour les modèles. `.obsidian` fait reconnaître le
/// dossier comme un vault par Obsidian.
pub const PARA_FOLDERS: &[&str] = &[
    "0_Inbox",
    "1_Projets",
    "2_Casquettes",
    "3_Ressources",
    "4_Archives",
    "_SYSTEM/Templates",
    ".obsidian",
];

#[derive(Debug, Clone)]
pub struct VaultHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
}

impl Vault {
    /// Ouvre le vault. `None` si le chemin est vide ou n'existe pas : le
    /// vault est un complément, son absence ne doit rien faire échouer.
    pub fn open(root: &str, folder: &str) -> Option<Self> {
        let root = PathBuf::from(root.trim());
        if root.as_os_str().is_empty() || !root.is_dir() {
            return None;
        }
        let folder = join_folder(&root, folder);
        Some(Vault {
            root,
            folder,
            origin: VaultOrigin::Configured,
            inbox_lock: Default::default(),
        })
    }

    /// Trouve et ouvre le vault. Chemin réglé (non vide) : pris tel quel,
    /// `None` s'il n'existe pas. Vide = automatique : le vault d'Obsidian de
    /// la machine (registre `obsidian.json`), sinon un vault propre à Jimy
    /// dans son dossier de données (`<data>\vault`), créé avec le squelette
    /// PARA. Rien n'est enregistré dans la config : un `config.json`
    /// illisible retombe aux défauts (piège 91), le sauver ici l'écraserait.
    pub fn locate(configured: &str, data_dir: &Path, folder: &str) -> Option<Self> {
        if !configured.trim().is_empty() {
            return Vault::open(configured, folder);
        }
        let registered = obsidian_registry().and_then(|raw| pick_registered_vault(&raw));
        let (root, origin) = match registered {
            Some(root) => (root, VaultOrigin::Obsidian),
            None => {
                let own = data_dir.join("vault");
                if let Err(error) = create_para_vault(&own, folder) {
                    log::warn!("[vault] création du vault propre impossible ({}) : {error}", own.display());
                    return None;
                }
                (own, VaultOrigin::Own)
            }
        };
        let mut vault = Vault::open(&root.to_string_lossy(), folder)?;
        vault.origin = origin;
        Some(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn origin(&self) -> VaultOrigin {
        self.origin
    }

    /// Recherche plein texte dans tout le vault. Les mots de la requête sont
    /// normalisés (minuscules, accents retirés) ; une note est retenue dès
    /// qu'un mot y figure, et classée par couverture : tous les mots trouvés
    /// passe avant un seul, le titre compte davantage que le corps.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<VaultHit> {
        let limit = limit.clamp(1, 20);
        let mots: Vec<Vec<char>> = normaliser(query)
            .split_whitespace()
            .filter(|m| m.chars().count() >= 2)
            .map(|m| m.chars().collect())
            .collect();
        if mots.is_empty() {
            return Vec::new();
        }

        let mut chemins = Vec::new();
        collecter_notes(&self.root, &mut chemins);
        // Les captures archivées doublonnent les notes triées : hors recherche.
        let archive = join_folder(&self.root, INBOX_ARCHIVE);
        chemins.retain(|c| !c.starts_with(&archive));
        let mut hits: Vec<VaultHit> = Vec::new();
        for chemin in chemins {
            let Ok(contenu) = tokio::fs::read_to_string(&chemin).await else {
                continue;
            };
            let titre = titre_note(&chemin);
            let titre_norm: Vec<char> = normaliser(&titre).chars().collect();
            let corps_norm: Vec<char> = normaliser(&contenu).chars().collect();

            let mut score = 0.0f32;
            let mut position: Option<usize> = None;
            for (rang, mot) in mots.iter().enumerate() {
                let occurrences_titre = compter(&titre_norm, mot);
                let occurrences = compter(&corps_norm, mot);
                if occurrences + occurrences_titre == 0 {
                    continue;
                }
                if position.is_none() && occurrences > 0 {
                    position = position_de(&corps_norm, mot);
                }
                // Poids : titre ×4, premier mot de la requête +1, récurrence
                // plafonnée pour ne pas favoriser les notes bavardes.
                let mut poids = if occurrences_titre > 0 { 4.0 } else { 1.0 };
                if rang == 0 {
                    poids += 1.0;
                }
                score += poids + (occurrences as f32 - 1.0).min(3.0) * 0.5;
            }
            if score > 0.0 {
                let relatif = chemin
                    .strip_prefix(&self.root)
                    .unwrap_or(&chemin)
                    .display()
                    .to_string();
                // PARA : un projet actif passe avant une archive.
                let score = score * poids_para(&relatif);
                hits.push(VaultHit {
                    path: relatif,
                    title: titre,
                    snippet: extrait(&contenu, position),
                    score,
                });
            }
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        hits
    }

    /// Lit une note du vault, identifiée par son chemin relatif à la racine
    /// (renvoyé par `search`) — jamais en absolu : on reste dans le vault.
    pub async fn read(&self, relatif: &str) -> Result<String> {
        let cible = self
            .root
            .join(relatif.trim().trim_start_matches(['/', '\\']));
        if !cible.starts_with(&self.root) {
            return Err(Error::Tool("chemin hors du vault".into()));
        }
        let contenu = tokio::fs::read_to_string(&cible)
            .await
            .map_err(|e| Error::Tool(format!("lecture impossible : {e}")))?;
        Ok(couper(&contenu, MAX_OUTPUT_CHARS))
    }

    /// Capture un souvenir dans l'inbox de Jimy : **une puce** dans le fichier
    /// du jour (`0_Inbox/Jimmy/AAAA-MM-JJ.md`), pas un fichier par souvenir
    /// (141 notes d'une phrase en six jours avant le 9 octobre). Aucune
    /// décision de rangement ici : le tri (`memory::sort`) les range en PARA.
    /// Un souvenir déjà capturé le même jour n'est pas répété.
    pub async fn remember(&self, kind: &str, content: &str) -> Result<PathBuf> {
        let _garde = self.inbox_lock.lock().await;
        tokio::fs::create_dir_all(&self.folder)
            .await
            .map_err(|e| Error::Tool(format!("dossier du vault inaccessible : {e}")))?;
        let horodatage = horodatage_local();
        let (jour, heure) = horodatage.split_once('T').unwrap_or((&horodatage, ""));
        let heure = heure.get(..5).unwrap_or_default().replace('-', ":");
        let chemin = self.folder.join(format!("{jour}.md"));
        // Une capture tient sur une ligne.
        let texte = content.split_whitespace().collect::<Vec<_>>().join(" ");
        let existant = tokio::fs::read_to_string(&chemin).await.unwrap_or_default();
        if existant.lines().filter_map(parse_capture).any(|(_, t)| t == texte) {
            return Ok(chemin);
        }
        let mut note = if existant.trim().is_empty() {
            format!(
                "---\ntype: inbox\nsource: jimy\ndate: {jour}\n---\n\n# Captures du {jour}\n\n\
                 Souvenirs à trier : Jimy les range dans ses notes du vault (PARA).\n\n"
            )
        } else {
            existant
        };
        if !note.ends_with('\n') {
            note.push('\n');
        }
        note.push_str(&format!("- {heure} [{kind}] {texte}\n"));
        tokio::fs::write(&chemin, note)
            .await
            .map_err(|e| Error::Tool(format!("écriture impossible : {e}")))?;
        Ok(chemin)
    }

    /// Dossier de capture (absolu), lu par le tri.
    pub fn inbox(&self) -> &Path {
        &self.folder
    }

    /// Nombre de notes du vault (statistique de l'interface).
    pub fn count(&self) -> usize {
        let mut chemins = Vec::new();
        collecter_notes(&self.root, &mut chemins);
        chemins.len()
    }

    /// Dossier où Jimmy écrit, pour l'affichage dans l'interface.
    pub fn folder_display(&self) -> String {
        self.folder.display().to_string()
    }

    /// La note (chemin relatif au vault) est-elle une note de Jimmy ? Le
    /// vault est partagé avec l'utilisateur et d'autres agents : ses
    /// souvenirs sont son inbox et ses notes rangées (`Jimy_…`,
    /// `_SYSTEM/Jimy_Memory`, archive de l'inbox).
    pub fn is_own(&self, relative: &str) -> bool {
        // `folder` est absolu (racine + sous-dossier) : on compare le relatif.
        let own = self.folder.strip_prefix(&self.root).unwrap_or(&self.folder);
        is_own_note(relative, &own.to_string_lossy())
    }
}

/// Dossier des notes de Jimy qui ne relèvent d'aucun sujet PARA (profil,
/// méthodes, leçons).
pub const OWN_MEMORY_DIR: &str = "_SYSTEM/Jimy_Memory";
/// Préfixe des notes de Jimy rangées dans les dossiers PARA.
pub const OWN_NOTE_PREFIX: &str = "Jimy_";

/// Note de Jimy ? Dans son inbox (`folder`), nommée `Jimy_…`, dans
/// `_SYSTEM/Jimy_Memory` ou dans l'archive de son inbox. Séparateurs et casse
/// ignorés (`0_Inbox\Jimmy\x.md` comme `0_inbox/jimmy/x.md`).
pub fn is_own_note(relative: &str, folder: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_matches('/').to_lowercase();
    let dans = |path: &str, dossier: &str| {
        !dossier.is_empty() && (path == dossier || path.starts_with(&format!("{dossier}/")))
    };
    let path = norm(relative);
    let nom = path.rsplit('/').next().unwrap_or_default();
    dans(&path, &norm(folder))
        || nom.starts_with(&OWN_NOTE_PREFIX.to_lowercase())
        || dans(&path, &norm(OWN_MEMORY_DIR))
        || dans(&path, &norm(INBOX_ARCHIVE))
}

/// Une ligne de capture de l'inbox (`- HH:MM [genre] texte`) → (genre, texte).
pub fn parse_capture(line: &str) -> Option<(String, String)> {
    let reste = line.trim().strip_prefix("- ")?;
    let ouvre = reste.find('[')?;
    let ferme = reste[ouvre..].find(']')? + ouvre;
    let texte = reste[ferme + 1..].trim();
    (!texte.is_empty()).then(|| (reste[ouvre + 1..ferme].trim().to_string(), texte.to_string()))
}

// ── Découverte du vault ──────────────────────────────────────────────────────

/// Contenu du registre des vaults d'Obsidian (`%APPDATA%\obsidian\obsidian.json`),
/// `None` si Obsidian n'a jamais été lancé sur la machine.
fn obsidian_registry() -> Option<String> {
    let appdata = std::env::var_os("APPDATA")?;
    std::fs::read_to_string(PathBuf::from(appdata).join("obsidian").join("obsidian.json")).ok()
}

/// Parmi les vaults du registre d'Obsidian qui existent encore sur le disque :
/// celui ouvert (`open`), sinon le plus récemment utilisé (`ts`).
pub fn pick_registered_vault(raw: &str) -> Option<PathBuf> {
    let json: serde_json::Value = serde_json::from_str(raw).ok()?;
    json.get("vaults")?
        .as_object()?
        .values()
        .filter_map(|entry| {
            let path = PathBuf::from(entry.get("path")?.as_str()?);
            let open = entry.get("open").and_then(|v| v.as_bool()).unwrap_or(false);
            let ts = entry.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
            path.is_dir().then_some((open, ts, path))
        })
        .max_by_key(|(open, ts, _)| (*open, *ts))
        .map(|(_, _, path)| path)
}

/// Crée (ou complète) un vault à l'architecture PARA, plus le dossier de
/// capture de Jimy. Idempotent : ne touche à aucune note existante.
pub fn create_para_vault(root: &Path, folder: &str) -> std::io::Result<()> {
    for dossier in PARA_FOLDERS {
        std::fs::create_dir_all(join_folder(root, dossier))?;
    }
    std::fs::create_dir_all(join_folder(root, folder))
}

/// Poids d'une note selon sa place dans PARA : projets actifs > casquettes >
/// ressources > le reste > archives (terminé ou inactif).
fn poids_para(relatif: &str) -> f32 {
    let racine = relatif.split(['/', '\\']).next().unwrap_or_default().to_lowercase();
    match racine.as_str() {
        "1_projets" => 1.3,
        "2_casquettes" => 1.15,
        "3_ressources" => 1.1,
        "4_archives" => 0.6,
        _ => 1.0,
    }
}

/// Noms de toutes les notes du vault (minuscules, sans extension) : sert à
/// vérifier qu'un lien `[[…]]` pointe vers une note qui existe.
pub(crate) fn note_names(root: &Path) -> std::collections::HashSet<String> {
    let mut chemins = Vec::new();
    collecter_notes(root, &mut chemins);
    chemins.iter().map(|c| titre_note(c).to_lowercase()).collect()
}

// ── Aides ────────────────────────────────────────────────────────────────────

/// Sous-dossier du vault ; les réglages acceptent `/` ou `\` comme
/// séparateurs. Vide = `0_Inbox/Jimmy`.
pub(crate) fn join_folder(root: &Path, folder: &str) -> PathBuf {
    let dossier = folder.trim();
    if dossier.is_empty() {
        return root.join("0_Inbox").join("Jimmy");
    }
    let mut acc = root.to_path_buf();
    for part in dossier.split(['/', '\\']).filter(|p| !p.is_empty()) {
        acc.push(part);
    }
    acc
}

/// Collecte récursive des notes Markdown, hors dossiers système (`.obsidian`,
/// `.trash`, `.claude`…) et notes trop grosses.
fn collecter_notes(racine: &Path, sortie: &mut Vec<PathBuf>) {
    let Ok(entrées) = std::fs::read_dir(racine) else {
        return;
    };
    for entrée in entrées.flatten() {
        let chemin = entrée.path();
        let Some(nom) = chemin.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if nom.starts_with('.') || nom == "node_modules" {
            continue;
        }
        if chemin.is_dir() {
            collecter_notes(&chemin, sortie);
        } else if nom.to_ascii_lowercase().ends_with(".md") {
            if let Ok(métadonnées) = entrée.metadata() {
                if métadonnées.len() <= MAX_NOTE_BYTES {
                    sortie.push(chemin);
                }
            }
        }
    }
}

/// Nom affichable : le nom de fichier sans extension.
fn titre_note(chemin: &Path) -> String {
    chemin
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string()
}

/// Minuscules et accents retirés, **caractère par caractère** : la longueur en
/// caractères est conservée, donc un index de caractère dans la version
/// normalisée reste valable dans l'original.
pub(crate) fn normaliser(texte: &str) -> String {
    texte
        .chars()
        .map(|c| {
            let minuscule = if c.is_ascii() {
                c.to_ascii_lowercase()
            } else {
                c.to_lowercase().next().unwrap_or(c)
            };
            match minuscule {
                'à' | 'á' | 'â' | 'ä' | 'ã' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ý' | 'ÿ' => 'y',
                'ç' => 'c',
                'ñ' => 'n',
                autre => autre,
            }
        })
        .collect()
}

/// Occurrences d'un motif dans un texte découpé en caractères.
fn compter(texte: &[char], motif: &[char]) -> usize {
    if motif.is_empty() || texte.len() < motif.len() {
        return 0;
    }
    texte
        .windows(motif.len())
        .filter(|fenêtre| *fenêtre == motif)
        .count()
}

/// Position (en caractères) de la première occurrence du motif.
fn position_de(texte: &[char], motif: &[char]) -> Option<usize> {
    if motif.is_empty() || texte.len() < motif.len() {
        return None;
    }
    texte
        .windows(motif.len())
        .position(|fenêtre| fenêtre == motif)
}

/// Extrait autour de la première correspondance (~220 caractères, coupé à un
/// espace), remonté au début de la ligne.
fn extrait(contenu: &str, position: Option<usize>) -> String {
    let caractères: Vec<char> = contenu.chars().collect();
    let début = position.unwrap_or(0).min(caractères.len());
    let départ = caractères[..début]
        .iter()
        .rposition(|c| *c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let morceau: String = caractères[départ..]
        .iter()
        .take(220)
        .collect();
    let coupé = match morceau.rfind(' ') {
        Some(i) if morceau.chars().count() >= 220 => morceau[..i].to_string(),
        _ => morceau,
    };
    coupé.trim().replace('\n', " ⏎ ")
}

/// Horodatage local au format nom de fichier, `AAAA-MM-JJTHH-MM-SS`.
///
/// Sans dépendance `chrono` : temps UTC + décalage de Paris, approximatif aux
/// heures de changement d'heure — suffisant pour ordonner des souvenirs.
const DÉCALAGE_PARIS: u64 = 2 * 60 * 60; // heure d'été (CEST)

pub(crate) fn horodatage_local() -> String {
    let durée = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    civil(durée.as_secs() + DÉCALAGE_PARIS)
}

/// Convertit un horodatage en texte civil (algorithme de Howard Hinnant).
fn civil(secondes: u64) -> String {
    const JOUR: u64 = 86_400;
    let jours = secondes / JOUR;
    let reste = secondes % JOUR;
    let (h, m, s) = (reste / 3600, (reste % 3600) / 60, reste % 60);
    let z = jours as i64 + 719_468;
    let ère = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - ère * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + ère * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mois = if mp < 10 { mp + 3 } else { mp - 9 };
    let année = if mois <= 2 { y + 1 } else { y };
    format!("{année:04}-{mois:02}-{d:02}T{h:02}-{m:02}-{s:02}")
}

/// Coupe un texte proprement à une limite de caractères.
fn couper(texte: &str, max: usize) -> String {
    if texte.chars().count() <= max {
        return texte.to_string();
    }
    let coupé: String = texte.chars().take(max).collect();
    format!("{coupé}\n[…note tronquée]")
}

// ── Règle de déclenchement ───────────────────────────────────────────────────

/// Verdict de la décision « faut-il consulter le vault ? ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usefulness {
    /// Non : demande courte et autonome, la mémoire locale suffit.
    No,
    /// Oui : la demande référence un contexte antérieur.
    Yes,
}

/// Formulation explicite de la règle de déclenchement, pour l'interface et
/// les logs. Volontairement simple et lisible : c'est une heuristique, pas un
/// classifieur.
pub fn should_consult(request: &str, min_chars: u32, has_context_markers: bool) -> Usefulness {
    if has_context_markers {
        return Usefulness::Yes;
    }
    if request.chars().count() as u32 >= min_chars {
        return Usefulness::Yes;
    }
    Usefulness::No
}

/// Marqueurs de référence au contexte antérieur, en français.
const CONTEXT_MARKERS: &[&str] = &[
    "la dernière fois", "la semaine dernière", "hier", "avant-hier", "mon projet", "mes projets",
    "mon infrastructure", "comme d'habitude", "tu te souviens", "souviens-toi", "on avait",
    "precedent", "précédent", "déjà fait", "deja fait", "mon setup", "ma config",
    "configuration actuelle", "ce que j'ai demandé", "ce que nous avons",
];

pub fn has_context_markers(request: &str) -> bool {
    let lowered = request.to_lowercase();
    CONTEXT_MARKERS.iter().any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_notes_de_jimmy_se_distinguent_des_notes_partagees() {
        assert!(is_own_note("0_Inbox\\Jimmy\\2026-10-04-souvenir.md", "0_Inbox/Jimmy"));
        assert!(is_own_note("0_inbox/jimmy/x.md", "0_Inbox/Jimmy"));
        // Journal d'un autre agent, ou un dossier au nom proche : partagé.
        assert!(!is_own_note("_SYSTEM\\Journal_Agent\\session_2026-09-26.md", "0_Inbox/Jimmy"));
        assert!(!is_own_note("0_Inbox/JimmyBis/x.md", "0_Inbox/Jimmy"));
        assert!(!is_own_note("x.md", ""));
    }

    /// Le dossier de Jimmy est stocké en absolu (racine + sous-dossier) :
    /// `is_own` doit comparer le chemin relatif.
    #[test]
    fn is_own_marche_avec_un_vrai_vault() {
        let racine = std::env::temp_dir().join(format!("jimmy-vault-own-{}", std::process::id()));
        std::fs::create_dir_all(&racine).unwrap();
        let vault = Vault::open(&racine.to_string_lossy(), "0_Inbox/Jimmy").expect("vault");
        assert!(vault.is_own("0_Inbox\\Jimmy\\souvenir.md"));
        assert!(!vault.is_own("_SYSTEM\\Journal_Agent\\session.md"));
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// Registre d'Obsidian : le vault ouvert gagne, puis le plus récent ; un
    /// vault disparu du disque est ignoré.
    #[test]
    fn le_registre_d_obsidian_designe_le_bon_vault() {
        let base = std::env::temp_dir().join(format!("jimmy-registre-{}", std::process::id()));
        let (ancien, recent) = (base.join("ancien"), base.join("recent"));
        std::fs::create_dir_all(&ancien).unwrap();
        std::fs::create_dir_all(&recent).unwrap();
        let disparu = base.join("disparu");
        let registre = |open_ancien: bool| {
            serde_json::json!({ "vaults": {
                "a": { "path": ancien, "ts": 100, "open": open_ancien },
                "b": { "path": recent, "ts": 200 },
                "c": { "path": disparu, "ts": 999, "open": true },
            }})
            .to_string()
        };
        assert_eq!(pick_registered_vault(&registre(true)), Some(ancien.clone()));
        assert_eq!(pick_registered_vault(&registre(false)), Some(recent.clone()));
        assert_eq!(pick_registered_vault(r#"{"vaults":{}}"#), None);
        assert_eq!(pick_registered_vault("pas du json"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Sans Obsidian : le vault propre reprend l'architecture PARA et le
    /// dossier de capture ; refaire la création ne casse rien.
    #[test]
    fn le_vault_propre_suit_l_architecture_para() {
        let racine = std::env::temp_dir().join(format!("jimmy-para-{}", std::process::id()));
        create_para_vault(&racine, "0_Inbox/Jimmy").unwrap();
        std::fs::write(racine.join("1_Projets").join("note.md"), "garde-moi").unwrap();
        create_para_vault(&racine, "0_Inbox/Jimmy").unwrap();
        for dossier in PARA_FOLDERS.iter().chain(["0_Inbox/Jimmy"].iter()) {
            assert!(join_folder(&racine, dossier).is_dir(), "{dossier} manquant");
        }
        assert_eq!(std::fs::read_to_string(racine.join("1_Projets").join("note.md")).unwrap(), "garde-moi");
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// Un chemin réglé est pris tel quel, même s'il n'existe pas (pas de
    /// repli silencieux sur un autre vault).
    #[test]
    fn un_chemin_regle_n_est_jamais_remplace() {
        let racine = std::env::temp_dir().join(format!("jimmy-regle-{}", std::process::id()));
        std::fs::create_dir_all(&racine).unwrap();
        let vault = Vault::locate(&racine.to_string_lossy(), &racine, "0_Inbox/Jimmy").expect("vault");
        assert_eq!(vault.origin(), VaultOrigin::Configured);
        assert_eq!(vault.root(), racine.as_path());
        assert!(Vault::locate(&racine.join("absent").to_string_lossy(), &racine, "").is_none());
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn demande_courte_sans_marqueur_ne_declenche_pas() {
        assert_eq!(should_consult("quelle heure est-il", 180, false), Usefulness::No);
    }

    #[test]
    fn demande_longue_declenche() {
        let long = "x".repeat(200);
        assert_eq!(should_consult(&long, 180, false), Usefulness::Yes);
    }

    #[test]
    fn reference_au_contexte_declenche() {
        assert_eq!(
            should_consult("reprends mon projet comme la dernière fois", 180, true),
            Usefulness::Yes
        );
        assert!(has_context_markers("tu te souviens de mon infra ?"));
        assert!(!has_context_markers("ouvre le dossier X"));
    }

    #[test]
    fn normalisation_conserve_la_longueur() {
        let source = "Résultat trouvé : café";
        assert_eq!(normaliser(source).chars().count(), source.chars().count());
        assert_eq!(normaliser(source), "resultat trouve : cafe");
    }

    #[test]
    fn normalise_les_majuscules_accentuees() {
        assert_eq!(normaliser("ÉCOUTE À Montreal"), "ecoute a montreal");
    }

    /// Capture : une puce par souvenir dans le fichier du jour, sans
    /// doublon ; plus un fichier par souvenir.
    #[tokio::test]
    async fn la_capture_ajoute_une_puce_au_fichier_du_jour() {
        let racine = std::env::temp_dir().join(format!("jimmy-capture-{}", std::process::id()));
        std::fs::create_dir_all(&racine).unwrap();
        let vault = Vault::open(&racine.to_string_lossy(), "0_Inbox/Jimmy").unwrap();
        let a = vault.remember("semantic", "L'utilisateur préfère\nles réponses courtes.").await.unwrap();
        let b = vault.remember("procedural", "Toujours compiler avec build.ps1.").await.unwrap();
        vault.remember("semantic", "L'utilisateur préfère les réponses courtes.").await.unwrap();
        assert_eq!(a, b, "un seul fichier par jour");
        assert_eq!(std::fs::read_dir(vault.inbox()).unwrap().count(), 1);
        let contenu = std::fs::read_to_string(&a).unwrap();
        let captures: Vec<_> = contenu.lines().filter_map(parse_capture).collect();
        assert_eq!(captures.len(), 2, "{contenu}");
        assert_eq!(captures[0], ("semantic".into(), "L'utilisateur préfère les réponses courtes.".into()));
        assert!(contenu.starts_with("---\ntype: inbox\nsource: jimy"), "{contenu}");
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// PARA : à texte égal, le projet actif passe devant l'archive.
    #[tokio::test]
    async fn la_recherche_favorise_les_projets_actifs() {
        let racine = std::env::temp_dir().join(format!("jimmy-para-poids-{}", std::process::id()));
        for (dossier, nom) in [("4_Archives/Vieux", "a.md"), ("1_Projets/Actif", "b.md"), ("3_Ressources/Theme", "c.md")] {
            let d = join_folder(&racine, dossier);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(nom), "déploiement du serveur").unwrap();
        }
        let vault = Vault::open(&racine.to_string_lossy(), "").unwrap();
        let ordre: Vec<String> = vault
            .search("déploiement serveur", 5)
            .await
            .into_iter()
            .map(|h| h.path.replace('\\', "/"))
            .collect();
        assert_eq!(ordre, ["1_Projets/Actif/b.md", "3_Ressources/Theme/c.md", "4_Archives/Vieux/a.md"]);
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn les_notes_rangees_de_jimy_sont_les_siennes() {
        assert!(is_own_note("1_Projets/CRM_Ultimate/Jimy_CRM_Ultimate.md", "0_Inbox/Jimmy"));
        assert!(is_own_note("_SYSTEM\\Jimy_Memory\\Profil.md", "0_Inbox/Jimmy"));
        assert!(is_own_note("4_Archives/Jimy/Inbox/2026-10-09.md", "0_Inbox/Jimmy"));
        assert!(!is_own_note("1_Projets/CRM_Ultimate/README.md", "0_Inbox/Jimmy"));
        assert!(!is_own_note("_SYSTEM/Journal_Agent/session.md", "0_Inbox/Jimmy"));
    }

    #[test]
    fn horodatage_formatte() {
        // 2026-10-03 00:00:00 UTC = 1 790 985 600 s ; +12 h 34 min 5 s.
        let horodatage = civil(1_790_985_600 + 12 * 3600 + 34 * 60 + 5);
        assert!(horodatage.starts_with("2026-10-03T12-34-05"), "{horodatage}");
    }

    #[test]
    fn extrait_autour_de_la_correspondance() {
        let contenu = "ligne un\nligne deux avec mot-clé\nligne trois";
        let position = position_de(
            &normaliser(contenu).chars().collect::<Vec<_>>(),
            &"mot".chars().collect::<Vec<_>>(),
        );
        let morceau = extrait(contenu, position);
        assert!(morceau.contains("ligne deux"), "{morceau}");
    }
}
