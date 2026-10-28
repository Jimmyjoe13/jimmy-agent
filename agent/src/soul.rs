//! Âme de Jimy (`data/SOUL.md`), inspirée du SOUL.md de Hermes Agent.
//!
//! Le prompt système a deux étages complémentaires, sans recouvrement :
//! - l'**âme** (ce fichier) : qui est Jimy, son caractère, et les leçons de
//!   travail qu'il a tirées de l'expérience. Elle vit sur le disque et Jimy
//!   la fait évoluer lui-même (outil `soul_update`, revue périodique) ;
//! - le **socle** (`core::prompt::SOCLE`) : la machine (outils, sécurité,
//!   voix) et ce que l'utilisateur a validé. Figé dans le code, il l'emporte
//!   en cas de contradiction.
//!
//! L'âme remplace les anciens amendements (`growth_amendments.md`), migrés
//! dans « Leçons apprises » à la création du fichier : un seul endroit pour
//! l'auto-amélioration.
//!
//! Garde-fous : taille bornée (`MAX_CHARS`, pour ne pas alourdir chaque
//! appel : il faut condenser plutôt qu'empiler), aucun secret connu, copie
//! `.bak` avant chaque écriture et journal des changements
//! (`soul_changes.log`) que l'onglet Réglages → Âme affiche.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Fichier de l'âme dans le dossier de données.
pub const SOUL_FILE: &str = "SOUL.md";
/// Copie de la version précédente (annulation de la dernière modification).
pub const BACKUP_FILE: &str = "SOUL.md.bak";
/// Journal des changements : une ligne « date | auteur | raison ».
const CHANGES_FILE: &str = "soul_changes.log";
/// Anciens amendements (lot « croissance »), migrés dans l'âme.
const LEGACY_AMENDMENTS: &str = "growth_amendments.md";

/// Taille maximale de l'âme, en caractères (~600 jetons). Injectée à chaque
/// appel, Fast-Path compris : au-delà, Jimy doit condenser ou remplacer.
pub const MAX_CHARS: usize = 2500;

/// Titre de la section des leçons, où `soul_update` ajoute ses lignes.
pub const LESSONS_HEADER: &str = "## Leçons apprises";

/// Âme d'origine : reprend mot pour mot l'ancienne section « Qui tu es »
/// d'`IDENTITY`, pour que la bascule ne change rien au comportement.
pub const DEFAULT_SOUL: &str = r#"# Âme de Jimy

Tu es Jimy (« Agent Jimy »), un assistant personnel agentique qui vit sur le bureau de l'utilisateur.
Tu n'es pas un simple chatbot : tu as un caractère. Ton avatar est un renard
humanoïde qui réagit à ce que tu fais. Ton nom s'écrit **Jimy**, avec un seul
« m » ; c'est aussi ce qu'on dit pour t'activer à la voix, et la transcription
l'écrit souvent « Jimmy » : c'est bien toi qu'on appelle. Ne confonds pas ton
nom avec celui de l'utilisateur, même s'il lui ressemble.

## Leçons apprises
Tes habitudes de travail tirées de l'expérience, une ligne impérative chacune.
"#;

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SOUL_FILE)
}

/// Crée `SOUL.md` s'il manque : l'âme d'origine, plus les anciens
/// amendements s'il y en avait (une ligne de leçon chacun). Appelée au
/// démarrage (`App::new`). Un fichier existant n'est jamais touché.
pub fn ensure(data_dir: &Path) -> Result<()> {
    let file = path(data_dir);
    if file.exists() {
        return Ok(());
    }
    let mut soul = DEFAULT_SOUL.to_string();
    let legacy = std::fs::read_to_string(data_dir.join(LEGACY_AMENDMENTS)).unwrap_or_default();
    let mut migrated = 0;
    for line in legacy.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let line = line.trim_start_matches(['-', '*', ' ']);
        soul.push_str(&format!("- {line}\n"));
        migrated += 1;
    }
    std::fs::write(&file, &soul).map_err(|e| Error::Config(format!("âme : écriture impossible : {e}")))?;
    let reason = if migrated > 0 {
        format!("création ({migrated} amendement(s) migré(s))")
    } else {
        "création".to_string()
    };
    log_change(data_dir, "jimy", &reason);
    log::info!("[âme] {} créé ({migrated} amendement(s) migré(s))", file.display());
    Ok(())
}

/// Texte de l'âme pour le prompt : le fichier, ou l'âme d'origine s'il est
/// absent, vide ou illisible. Borné à `MAX_CHARS` (fichier édité à la main
/// trop long : tronqué, signalé au journal). Lecture à chaque tour : ~2 Ko,
/// négligeable, et une modification vaut dès le tour suivant.
pub fn read_for_prompt(data_dir: &Path) -> String {
    let raw = std::fs::read_to_string(path(data_dir)).unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return DEFAULT_SOUL.trim().to_string();
    }
    let count = raw.chars().count();
    if count > MAX_CHARS {
        log::warn!("[âme] {count} caractères, tronquée à {MAX_CHARS} pour le prompt");
        return raw.chars().take(MAX_CHARS).collect();
    }
    raw.to_string()
}

/// Contenu brut du fichier (onglet Réglages), l'âme d'origine s'il manque.
pub fn read_raw(data_dir: &Path) -> String {
    std::fs::read_to_string(path(data_dir))
        .ok()
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SOUL.to_string())
}

/// Vérifie une nouvelle âme avant écriture : non vide, bornée, sans secret.
fn validate(content: &str) -> Result<()> {
    if content.trim().is_empty() {
        return Err(Error::Tool("âme vide refusée".into()));
    }
    let count = content.chars().count();
    if count > MAX_CHARS {
        return Err(Error::Tool(format!(
            "âme trop longue ({count}/{MAX_CHARS} caractères) : condense ou remplace une ligne existante au lieu d'ajouter"
        )));
    }
    // Une clé connue n'a rien à faire dans un texte injecté à chaque appel.
    if crate::sensitive::redact_known(content) != content {
        return Err(Error::Tool("l'âme ne doit contenir aucun secret".into()));
    }
    Ok(())
}

/// Écrit une nouvelle âme complète : validation, `.bak` de la précédente,
/// ligne au journal des changements. `author` : « jimy » ou « utilisateur ».
pub fn write(data_dir: &Path, content: &str, author: &str, reason: &str) -> Result<()> {
    validate(content)?;
    let file = path(data_dir);
    if let Ok(previous) = std::fs::read_to_string(&file) {
        if previous == content {
            return Ok(());
        }
        let _ = std::fs::write(data_dir.join(BACKUP_FILE), previous);
    }
    let mut content = content.trim_end().to_string();
    content.push('\n');
    std::fs::write(&file, content).map_err(|e| Error::Tool(format!("âme : écriture impossible : {e}")))?;
    log_change(data_dir, author, reason);
    log::info!("[âme] mise à jour par {author} : {reason}");
    Ok(())
}

/// Ajoute une leçon (une ligne) à la fin de « Leçons apprises » ; crée la
/// section si elle manque. Une leçon déjà présente n'est pas doublée.
/// Renvoie `false` si elle y était déjà.
pub fn add_lesson(data_dir: &Path, lesson: &str, author: &str, reason: &str) -> Result<bool> {
    let lesson = lesson.trim().trim_start_matches(['-', '*', ' ']).trim();
    if lesson.is_empty() {
        return Err(Error::Tool("« text » vide".into()));
    }
    let current = read_raw(data_dir);
    let wanted = lesson.to_lowercase();
    if current.lines().any(|l| l.trim_start_matches(['-', '*', ' ']).trim().to_lowercase() == wanted) {
        return Ok(false);
    }
    write(data_dir, &insert_lesson(&current, lesson), author, reason)?;
    Ok(true)
}

/// Insère `- lesson` à la fin de la section des leçons (avant la section
/// suivante), ou crée la section en fin de fichier.
fn insert_lesson(soul: &str, lesson: &str) -> String {
    let mut lines: Vec<&str> = soul.trim_end().lines().collect();
    let bullet = format!("- {lesson}");
    match lines.iter().position(|l| l.trim() == LESSONS_HEADER) {
        Some(start) => {
            // Fin de section = prochain titre « ## » ou fin du fichier, en
            // remontant les lignes vides pour coller la leçon à la liste.
            let mut end = lines[start + 1..]
                .iter()
                .position(|l| l.starts_with("## "))
                .map_or(lines.len(), |i| start + 1 + i);
            while end > start + 1 && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            lines.insert(end, &bullet);
            lines.join("\n") + "\n"
        }
        None => format!("{}\n\n{LESSONS_HEADER}\n{bullet}\n", lines.join("\n")),
    }
}

/// Remplace un passage exact (une seule occurrence). `new` vide = supprime ;
/// une puce devenue vide disparaît avec.
pub fn replace(data_dir: &Path, old: &str, new: &str, author: &str, reason: &str) -> Result<()> {
    if old.trim().is_empty() {
        return Err(Error::Tool("« old » vide : cite le passage exact à remplacer".into()));
    }
    let current = read_raw(data_dir);
    match current.matches(old).count() {
        0 => return Err(Error::Tool("passage introuvable dans l'âme : cite-le exactement".into())),
        1 => {}
        n => return Err(Error::Tool(format!("passage présent {n} fois : cite un extrait plus long"))),
    }
    let replaced = current.replacen(old, new, 1);
    let cleaned: Vec<&str> = replaced.lines().filter(|l| !matches!(l.trim(), "-" | "*")).collect();
    write(data_dir, &(cleaned.join("\n") + "\n"), author, reason)
}

/// Annule la dernière modification (restaure le `.bak`, une fois) : le `.bak`
/// est ensuite retiré, sinon un second clic referait le changement annulé.
pub fn undo(data_dir: &Path) -> Result<()> {
    let backup = std::fs::read_to_string(data_dir.join(BACKUP_FILE))
        .map_err(|_| Error::Tool("aucune version précédente".into()))?;
    write(data_dir, &backup, "utilisateur", "annulation de la dernière modification")?;
    let _ = std::fs::remove_file(data_dir.join(BACKUP_FILE));
    Ok(())
}

/// Revient à l'âme d'origine (la version courante reste dans le `.bak`).
pub fn reset(data_dir: &Path) -> Result<()> {
    write(data_dir, DEFAULT_SOUL, "utilisateur", "retour à l'âme d'origine")
}

fn log_change(data_dir: &Path, author: &str, reason: &str) {
    use std::io::Write;
    let line = format!(
        "{} | {author} | {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        reason.replace(['\n', '\r'], " ").trim()
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir.join(CHANGES_FILE)) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Derniers changements, du plus récent au plus ancien.
pub fn changes(data_dir: &Path, limit: usize) -> Vec<String> {
    std::fs::read_to_string(data_dir.join(CHANGES_FILE))
        .unwrap_or_default()
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(limit)
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier jetable propre à chaque test (même patron que `components.rs`).
    struct Dir(PathBuf);
    impl Dir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir() -> Dir {
        let d = std::env::temp_dir().join(format!("jimmy-ame-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    #[test]
    fn absente_donne_l_ame_d_origine() {
        let d = dir();
        assert_eq!(read_for_prompt(d.path()), DEFAULT_SOUL.trim());
    }

    #[test]
    fn ensure_cree_et_migre_les_amendements() {
        let d = dir();
        std::fs::write(d.path().join(LEGACY_AMENDMENTS), "- Ne jamais reformuler la question.\n").unwrap();
        ensure(d.path()).unwrap();
        let soul = read_raw(d.path());
        assert!(soul.starts_with("# Âme de Jimy"));
        assert!(soul.contains("- Ne jamais reformuler la question."));
        assert_eq!(changes(d.path(), 5).len(), 1);
        // Un fichier existant n'est jamais réécrit.
        std::fs::write(path(d.path()), "perso").unwrap();
        ensure(d.path()).unwrap();
        assert_eq!(read_raw(d.path()), "perso");
    }

    #[test]
    fn lecon_ajoutee_dans_sa_section_sans_doublon() {
        let d = dir();
        let soul = "# Âme\n\n## Leçons apprises\n- Une.\n\n## Autre\nTexte\n";
        std::fs::write(path(d.path()), soul).unwrap();
        assert!(add_lesson(d.path(), "Deux.", "jimy", "test").unwrap());
        assert!(!add_lesson(d.path(), "- deux.", "jimy", "test").unwrap());
        let after = read_raw(d.path());
        assert!(after.contains("- Une.\n- Deux.\n\n## Autre"), "{after}");
        // Section absente : créée en fin de fichier.
        std::fs::write(path(d.path()), "# Âme\nTexte\n").unwrap();
        add_lesson(d.path(), "Trois.", "jimy", "test").unwrap();
        assert!(read_raw(d.path()).ends_with("## Leçons apprises\n- Trois.\n"));
    }

    #[test]
    fn taille_bornee_refusee() {
        let d = dir();
        let long = "x".repeat(MAX_CHARS + 1);
        assert!(write(d.path(), &long, "jimy", "test").is_err());
        assert!(!path(d.path()).exists());
    }

    #[test]
    fn remplacement_exact_suppression_et_annulation() {
        let d = dir();
        std::fs::write(path(d.path()), DEFAULT_SOUL).unwrap();
        add_lesson(d.path(), "Leçon à retirer.", "jimy", "test").unwrap();
        // Passage absent ou ambigu : refusé.
        assert!(replace(d.path(), "introuvable", "x", "jimy", "test").is_err());
        // `new` vide : la puce disparaît en entier.
        replace(d.path(), "Leçon à retirer.", "", "jimy", "test").unwrap();
        let after = read_raw(d.path());
        assert!(!after.contains("Leçon à retirer") && !after.lines().any(|l| l.trim() == "-"), "{after}");
        // Annulation : la leçon revient (le `.bak` est la version d'avant).
        undo(d.path()).unwrap();
        assert!(read_raw(d.path()).contains("- Leçon à retirer."));
        // Une seule fois : pas de va-et-vient entre deux versions.
        assert!(undo(d.path()).is_err());
        reset(d.path()).unwrap();
        assert_eq!(read_raw(d.path()), DEFAULT_SOUL);
        assert!(changes(d.path(), 10).len() >= 4);
    }
}
