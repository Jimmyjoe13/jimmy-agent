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
}

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
        let dossier = folder.trim();
        let folder = if dossier.is_empty() {
            root.join("0_Inbox").join("Jimmy")
        } else {
            // Les réglages acceptent `/` ou `\` comme séparateurs.
            let mut acc = root.clone();
            for part in Path::new(dossier) {
                acc.push(part);
            }
            acc
        };
        Some(Vault { root, folder })
    }

    pub fn root(&self) -> &Path {
        &self.root
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

    /// Écrit un souvenir dans le dossier de Jimmy : une note par souvenir,
    /// datée, avec un frontmatter minimal que l'utilisateur relit comme une
    /// note ordinaire (et peut trier, lier, archiver).
    pub async fn remember(&self, kind: &str, content: &str) -> Result<PathBuf> {
        tokio::fs::create_dir_all(&self.folder)
            .await
            .map_err(|e| Error::Tool(format!("dossier du vault inaccessible : {e}")))?;
        let horodatage = horodatage_local();
        let chemin = self.folder.join(format!(
            "{}-{}.md",
            horodatage,
            slug(content, 6)
        ));
        let note = format!(
            "---\ntype: {kind}\nsource: jimmy\ndate: {date}\n---\n\n{content}\n",
            date = horodatage.split('T').next().unwrap_or_default(),
        );
        tokio::fs::write(&chemin, note)
            .await
            .map_err(|e| Error::Tool(format!("écriture impossible : {e}")))?;
        Ok(chemin)
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
    /// vault est partagé avec l'utilisateur et d'autres agents : seules les
    /// notes de son dossier sont ses souvenirs.
    pub fn is_own(&self, relative: &str) -> bool {
        // `folder` est absolu (racine + sous-dossier) : on compare le relatif.
        let own = self.folder.strip_prefix(&self.root).unwrap_or(&self.folder);
        is_own_note(relative, &own.to_string_lossy())
    }
}

/// `relative` est-il dans `folder` ? Séparateurs et casse ignorés
/// (`0_Inbox\Jimmy\x.md` comme `0_inbox/jimmy/x.md`).
pub fn is_own_note(relative: &str, folder: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_matches('/').to_lowercase();
    let folder = norm(folder);
    let path = norm(relative);
    !folder.is_empty() && (path == folder || path.starts_with(&format!("{folder}/")))
}

// ── Aides ────────────────────────────────────────────────────────────────────

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
fn normaliser(texte: &str) -> String {
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

fn horodatage_local() -> String {
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

/// Slug d'un texte : premiers mots significatifs, accents retirés, séparés
/// par des tirets. Pour un nom de fichier.
fn slug(texte: &str, mots_max: usize) -> String {
    normaliser(texte)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|m| !m.is_empty() && m.chars().count() > 1)
        .take(mots_max)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
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

    #[test]
    fn slug_lisible() {
        assert_eq!(
            slug("L'utilisateur préfère des réponses courtes !", 4),
            "utilisateur-prefere-des-reponses"
        );
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
