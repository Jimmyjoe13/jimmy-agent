//! Tri de l'inbox de Jimy vers le vault, selon l'architecture PARA.
//!
//! Capture et rangement sont séparés, comme le veut la méthode du vault
//! (« 0_Inbox : capture rapide, tri par lots ») : `Vault::remember` ajoute
//! une puce au fichier du jour, ce module range les captures **par lots**.
//!
//! 1. Les fichiers de l'inbox sont mis de côté (`.en-tri`, sous verrou : une
//!    capture concurrente repart dans un fichier neuf, rien ne se perd).
//! 2. Un appel au modèle par lot classe chaque souvenir : projet, casquette,
//!    ressource (avec le dossier PARA, existant ou nouveau, nommé comme les
//!    autres), profil, méthode, leçon, journal, ou à ignorer.
//! 3. Chaque note cible reçoit ses souvenirs **par fusion** (un appel par
//!    note) : doublons retirés, contradictions remplacées, sections claires.
//!    Jimy n'écrit que dans ses notes (`Jimy_…`, `_SYSTEM/Jimy_Memory`,
//!    journal `Jimy_AAAA-MM`) et met des liens vers celles de l'utilisateur.
//! 4. Les fichiers triés partent dans `4_Archives/Jimy/Inbox` (« on archive,
//!    on ne supprime pas »).
//!
//! Garde-fous : une fusion qui ferait fondre la note (< 70 % de sa taille)
//! est rejetée ; la version précédente est copiée dans `.jimy-historique`
//! (dossier caché, hors recherche) ; un souvenir non rangé (classement
//! illisible, fusion refusée) retourne dans l'inbox pour le tri suivant.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Deserialize;

use crate::core::types::Message;
use crate::error::{Error, Result};
use crate::memory::learn::{extract_json_block, SHORT_RETRY};
use crate::memory::vault::{self, Vault, INBOX_ARCHIVE, OWN_MEMORY_DIR, OWN_NOTE_PREFIX};
use crate::providers::llm::LlmClient;

/// Nombre de souvenirs en attente qui déclenche un tri (sinon : dès qu'il
/// reste des captures d'un jour passé, soit un tri par jour au plus).
pub const SORT_THRESHOLD: usize = 10;
/// Souvenirs classés par appel au modèle.
const BATCH: usize = 25;
/// Sous-dossier (caché) de l'inbox où les fichiers attendent pendant le tri.
const STAGING: &str = ".en-tri";
/// Copies des notes avant réécriture, à la racine du vault (caché).
const HISTORY_DIR: &str = ".jimy-historique";
/// Le raisonnement caché compte dans `max_tokens` (piège 70).
const MAX_TOKENS: u32 = 16_384;
/// Une fusion doit garder au moins 70 % de la taille de la note.
const MIN_KEEP_RATIO: f32 = 0.7;
/// Après un échec, pas de nouvel essai avant 30 min (chaque tour de
/// conversation relancerait sinon un appel voué à l'échec).
const RETRY_AFTER_SECS: u64 = 30 * 60;

static SORTING: AtomicBool = AtomicBool::new(false);
static LAST_FAILURE: AtomicU64 = AtomicU64::new(0);

const CLASSIFIER: &str = r#"Tu ranges les souvenirs de Jimy, l'assistant personnel de l'utilisateur, dans son vault Obsidian organisé selon la méthode PARA.

Destinations possibles :
- "projet" : lié à un projet précis, avec un objectif (dossier dans 1_Projets).
- "casquette" : une responsabilité continue de l'utilisateur, sans fin prévue (dossier dans 2_Casquettes).
- "ressource" : une connaissance ou une procédure technique réutilisable, sur un thème (dossier dans 3_Ressources).
- "profil" : qui est l'utilisateur, ses préférences stables.
- "methode" : une règle de travail entre Jimy et l'utilisateur.
- "lecon" : un échec rencontré et la bonne façon de faire.
- "journal" : un épisode qui peut servir d'exemple, sans sujet durable.
- "ignorer" : sans valeur durable (test, essai, vérification ponctuelle, politesse, détail sans portée).

Pour "projet", "casquette" et "ressource", "dossier" est le nom EXACT d'un dossier existant listé ci-dessous dès que le sujet est le même, même sous un autre nom (un dépôt, un nom de code, une variante d'écriture). Ne crée un nouveau dossier que pour un sujet clairement absent de la liste, avec un nom au même format que les existants (Mots_Avec_Majuscules, sans accents ni espaces).

Tout ce qui concerne les tests de l'interface de Jimy (fichiers de test, « test travaux », sessions de test) et les tentatives sans résultat (fichier introuvable, session interrompue) est à "ignorer".

Réponds uniquement par un tableau JSON, un élément par souvenir :
[{"n": 1, "dest": "projet", "dossier": "Nom_Du_Dossier"}, {"n": 2, "dest": "profil"}]"#;

const FUSION: &str = r#"Tu tiens une note Obsidian de la mémoire de Jimy, l'assistant personnel de l'utilisateur. On te donne la note actuelle et de nouveaux éléments : rends la note complète, à jour.

Règles :
- N'invente RIEN : n'écris que ce que disent la note actuelle et les nouveaux éléments. Aucune technologie, aucun outil, aucun site, aucune étape supposés.
- Garde tout ce qui reste vrai. Retire les doublons. Si un élément contredit la note, remplace l'ancien et date le changement.
- Commence par le titre `# …`, puis un résumé de deux ou trois lignes sur ce que la note contient.
- Range le contenu en sections `##` claires. Des phrases complètes et précises, compréhensibles seules dans six mois : qui, quoi, pourquoi. Pas de style télégraphique.
- Date les faits et décisions (AAAA-MM-JJ).
- Liens Obsidian : uniquement [[Nom]] vers une note listée dans « Notes de l'utilisateur », jamais vers une autre.
- Pour une note neuve, suis la structure de départ si elle est fournie, mais retire toute section pour laquelle tu n'as aucune information (jamais de « à définir » ni de texte de remplissage).
- Aucun secret, mot de passe, clé ou jeton.
- Réponds uniquement par le Markdown de la note, sans frontmatter et sans bloc de code englobant."#;

/// Un souvenir de l'inbox.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: String,
    pub text: String,
    /// `AAAA-MM-JJ`, vide si inconnu.
    pub date: String,
}

/// Bilan d'un tri, pour le journal.
#[derive(Debug, Default)]
pub struct SortReport {
    pub items: usize,
    pub notes: Vec<String>,
    pub ignored: usize,
    pub requeued: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Classed {
    n: usize,
    dest: String,
    #[serde(default)]
    dossier: String,
}

/// Note cible d'un groupe de souvenirs.
#[derive(Debug, Clone)]
struct Target {
    /// Chemin relatif au vault, séparateur `/`.
    relative: String,
    /// `projet`, `casquette`, `ressource`, `profil`, `methode`, `lecon`, `journal`.
    dest: String,
    title: String,
    /// Modèle du vault (`_SYSTEM/Templates`) pour une note neuve.
    template: Option<&'static str>,
}

/// Dossiers PARA existants (premier niveau).
#[derive(Debug, Default)]
pub struct Structure {
    pub projets: Vec<String>,
    pub casquettes: Vec<String>,
    pub ressources: Vec<String>,
}

impl Structure {
    pub fn read(root: &Path) -> Self {
        Structure {
            projets: subdirs(&root.join("1_Projets")),
            casquettes: subdirs(&root.join("2_Casquettes")),
            ressources: subdirs(&root.join("3_Ressources")),
        }
    }

    fn describe(&self) -> String {
        let liste = |noms: &[String]| if noms.is_empty() { "(aucun)".to_string() } else { noms.join(", ") };
        format!(
            "Dossiers existants :\n- 1_Projets : {}\n- 2_Casquettes : {}\n- 3_Ressources : {}",
            liste(&self.projets),
            liste(&self.casquettes),
            liste(&self.ressources)
        )
    }
}

// ── Déclenchement ────────────────────────────────────────────────────────────

/// Trie l'inbox si c'est le moment : au moins `SORT_THRESHOLD` souvenirs en
/// attente, ou des captures d'un jour passé. Un seul tri à la fois ; après un
/// échec, on attend `RETRY_AFTER_SECS`. Appelé après chaque apprentissage,
/// en tâche de fond.
pub async fn sort_if_needed(vault: &Vault, llm: &LlmClient, model: &str) -> Option<SortReport> {
    let files = inbox_files(vault.inbox());
    let today = today();
    let stale = files.iter().any(|f| stem(f) != today);
    let count: usize = files.iter().map(|f| items_of(&read(f)).len()).sum();
    if count == 0 || (!stale && count < SORT_THRESHOLD) {
        return None;
    }
    if now_secs().saturating_sub(LAST_FAILURE.load(Ordering::Relaxed)) < RETRY_AFTER_SECS {
        return None;
    }
    if SORTING.swap(true, Ordering::SeqCst) {
        return None;
    }
    // Libère le drapeau quoi qu'il arrive (erreur, panique).
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            SORTING.store(false, Ordering::SeqCst);
        }
    }
    let _reset = Reset;
    let started = std::time::Instant::now();
    match sort(vault, llm, model).await {
        Ok(report) => {
            log::info!(
                "[vault] tri en {} s : {} souvenir(s) → {} note(s) {:?} ({} ignoré(s), {} remis dans l'inbox)",
                started.elapsed().as_secs(),
                report.items,
                report.notes.len(),
                report.notes,
                report.ignored,
                report.requeued
            );
            Some(report)
        }
        Err(error) => {
            LAST_FAILURE.store(now_secs(), Ordering::Relaxed);
            log::warn!("[vault] tri abandonné, captures remises dans l'inbox : {error}");
            None
        }
    }
}

// ── Tri ──────────────────────────────────────────────────────────────────────

/// Trie toute l'inbox maintenant (voir l'en-tête du module).
pub async fn sort(vault: &Vault, llm: &LlmClient, model: &str) -> Result<SortReport> {
    let inbox = vault.inbox().to_path_buf();
    let staging = inbox.join(STAGING);

    // 1. Mise de côté, sous verrou : aucune capture ne tombe entre la lecture
    //    et le déplacement.
    let staged = {
        let _garde = vault.inbox_lock.lock().await;
        restore_staging(&inbox, &staging)?; // reste d'un tri interrompu
        std::fs::create_dir_all(&staging)?;
        let mut staged = Vec::new();
        for fichier in inbox_files(&inbox) {
            let cible = staging.join(fichier.file_name().unwrap_or_default());
            std::fs::rename(&fichier, &cible)?;
            staged.push(cible);
        }
        staged
    };
    let tous: Vec<Item> = staged.iter().flat_map(|f| items_of(&read(f))).collect();
    let total = tous.len();
    let items: Vec<Item> = tous.into_iter().filter(|i| !is_test_noise(&i.text)).collect();
    let mut report = SortReport {
        items: total,
        ignored: total - items.len(),
        ..Default::default()
    };

    // 2. Classement par lots. Un lot illisible est retenté en deux moitiés ;
    //    s'il échoue encore, seuls ses souvenirs retournent à l'inbox (premier
    //    essai réel : un seul lot raté annulait tout le tri). Aucun lot
    //    classé = tout revient à l'inbox, rien n'est écrit.
    let structure = Structure::read(vault.root());
    let mut classes: Vec<(Item, Option<Classed>)> = Vec::new();
    let mut lots_classes = 0;
    for lot in items.chunks(BATCH) {
        match classify(llm, model, &structure, lot).await {
            Ok(verdicts) => {
                lots_classes += 1;
                classes.extend(lot.iter().cloned().zip(verdicts));
            }
            Err(error) => {
                log::warn!("[vault] lot de {} illisible, nouvel essai en deux moitiés : {error}", lot.len());
                for moitie in lot.chunks(lot.len().div_ceil(2)) {
                    match classify(llm, model, &structure, moitie).await {
                        Ok(verdicts) => {
                            lots_classes += 1;
                            classes.extend(moitie.iter().cloned().zip(verdicts));
                        }
                        Err(_) => classes.extend(moitie.iter().cloned().map(|i| (i, None))),
                    }
                }
            }
        }
    }
    if lots_classes == 0 && !items.is_empty() {
        let _garde = vault.inbox_lock.lock().await;
        restore_staging(&inbox, &staging)?;
        return Err(Error::Tool("classement des souvenirs impossible".into()));
    }

    // 3. Regroupement par note cible.
    let mut groups: BTreeMap<String, (Target, Vec<Item>)> = BTreeMap::new();
    let mut requeue: Vec<Item> = Vec::new();
    for (item, verdict) in classes {
        match verdict {
            // Oublié par le modèle : retentera au prochain tri.
            None => requeue.push(item),
            Some(c) => match target_for(&c, &item, &structure) {
                None => report.ignored += 1,
                Some(target) => {
                    groups
                        .entry(target.relative.clone())
                        .or_insert_with(|| (target, Vec::new()))
                        .1
                        .push(item);
                }
            },
        }
    }

    // 4. Écriture, note par note : un échec n'arrête pas les autres.
    for (relative, (target, lot)) in groups {
        let result = if target.dest == "journal" {
            append_journal(vault.root(), &target, &lot)
        } else {
            merge_note(vault.root(), llm, model, &target, &lot).await
        };
        match result {
            Ok(()) => report.notes.push(relative),
            Err(error) => {
                log::warn!("[vault] {relative} non mise à jour : {error}");
                requeue.extend(lot);
            }
        }
    }

    // 5. Archive des fichiers triés, puis retour à l'inbox de ce qui n'a pas
    //    été rangé (daté du jour : la date d'origine reste dans l'archive).
    archive(vault.root(), &staged)?;
    let _ = std::fs::remove_dir(&staging);
    for item in &requeue {
        vault.remember(&item.kind, &item.text).await?;
    }
    report.requeued = requeue.len();
    Ok(report)
}

/// Classe un lot de souvenirs ; `None` pour un souvenir que le modèle a oublié.
async fn classify(
    llm: &LlmClient,
    model: &str,
    structure: &Structure,
    lot: &[Item],
) -> Result<Vec<Option<Classed>>> {
    let liste = lot
        .iter()
        .enumerate()
        .map(|(i, item)| format!("{}. [{}, {}] {}", i + 1, item.kind, item.date, item.text))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        Message::system(CLASSIFIER),
        Message::user(format!("{}\n\nSouvenirs à ranger :\n{liste}", structure.describe())),
    ];
    for attempt in 1..=2 {
        match llm.complete(model, &messages, MAX_TOKENS, Some(0.0)).await {
            Ok(raw) => {
                if let Some(verdicts) = parse_classes(&raw, lot.len()) {
                    return Ok(verdicts);
                }
                log::warn!(
                    "[vault] classement illisible (tentative {attempt}, {} caractères) : {}",
                    raw.chars().count(),
                    crate::sensitive::mask_text(&raw.chars().take(300).collect::<String>())
                );
            }
            Err(error) => log::warn!("[vault] classement en échec (tentative {attempt}) : {error}"),
        }
        if attempt < 2 {
            tokio::time::sleep(SHORT_RETRY).await;
        }
    }
    Err(Error::Tool("classement des souvenirs impossible".into()))
}

/// Réponse du classement → un verdict par souvenir (dans l'ordre du lot).
fn parse_classes(raw: &str, len: usize) -> Option<Vec<Option<Classed>>> {
    let json = extract_json_block(raw)?;
    let classes: Vec<Classed> = serde_json::from_str(&json).ok()?;
    let mut verdicts = vec![None; len];
    for c in classes {
        if (1..=len).contains(&c.n) {
            let index = c.n - 1;
            verdicts[index] = Some(c);
        }
    }
    Some(verdicts)
}

/// Note cible d'un souvenir classé ; `None` = à ignorer. Un projet, une
/// casquette ou une ressource sans nom de dossier utilisable part au journal.
fn target_for(c: &Classed, item: &Item, structure: &Structure) -> Option<Target> {
    let para = |racine: &str, existants: &[String], dest: &str, template: Option<&'static str>| {
        folder_name(&c.dossier, existants).map(|dossier| Target {
            relative: format!("{racine}/{dossier}/{OWN_NOTE_PREFIX}{dossier}.md"),
            dest: dest.into(),
            title: format!("Jimy — {}", dossier.replace('_', " ")),
            template,
        })
    };
    let own = |fichier: &str, dest: &str, title: &str| Target {
        relative: format!("{OWN_MEMORY_DIR}/{fichier}.md"),
        dest: dest.into(),
        title: title.into(),
        template: None,
    };
    let journal = || {
        let mois = item.date.get(..7).map(str::to_string).unwrap_or_else(|| today()[..7].to_string());
        Target {
            relative: format!("_SYSTEM/Journal_Agent/{OWN_NOTE_PREFIX}{mois}.md"),
            dest: "journal".into(),
            title: format!("Journal de Jimy — {mois}"),
            template: None,
        }
    };
    let target = match c.dest.as_str() {
        "ignorer" => return None,
        "projet" => para("1_Projets", &structure.projets, "projet", Some("TPL_Projet.md")),
        "casquette" => para("2_Casquettes", &structure.casquettes, "casquette", None),
        "ressource" => para("3_Ressources", &structure.ressources, "ressource", Some("TPL_Ressource.md")),
        "profil" => Some(own("Profil", "profil", "Profil de l'utilisateur")),
        "methode" => Some(own("Méthodes", "methode", "Méthodes de travail")),
        "lecon" => Some(own("Leçons", "lecon", "Leçons de l'expérience")),
        _ => None,
    };
    Some(target.unwrap_or_else(journal))
}

/// Nom de dossier PARA : un dossier existant s'il correspond (casse,
/// espaces et tirets ignorés), sinon un nom neuf au format du vault
/// (`Mots_Avec_Majuscules`, sans accents ni séparateurs de chemin).
pub fn folder_name(raw: &str, existants: &[String]) -> Option<String> {
    // Clé de comparaison : minuscules, sans séparateurs ni accents.
    let cle = |s: &str| {
        vault::normaliser(s)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };
    let demande = cle(raw);
    if let Some(existant) = existants.iter().find(|e| cle(e) == demande) {
        return Some(existant.clone());
    }
    // Même sujet sous un nom allongé ou raccourci (`JimmyAgentPersonnel` →
    // `Jimmy_Agent`, observé au premier tri réel) : préfixe commun d'au
    // moins 5 caractères, le plus long gagne.
    if let Some(existant) = existants
        .iter()
        .filter(|e| {
            let e = cle(e);
            e.len() >= 5 && demande.len() >= 5 && (demande.starts_with(&e) || e.starts_with(&demande))
        })
        .max_by_key(|e| cle(e).len())
    {
        return Some(existant.clone());
    }
    let mut nom = String::new();
    for c in raw.trim().chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            c
        } else if c.is_whitespace() {
            '_'
        } else {
            // Lettre accentuée → lettre de base, casse conservée.
            match vault::normaliser(&c.to_string()).chars().next() {
                Some(base) if base.is_ascii_alphanumeric() => {
                    if c.is_uppercase() {
                        base.to_ascii_uppercase()
                    } else {
                        base
                    }
                }
                _ => continue,
            }
        };
        if !(c == '_' && (nom.is_empty() || nom.ends_with('_'))) {
            nom.push(c);
        }
    }
    let nom: String = nom.trim_matches('_').chars().take(60).collect();
    (!nom.is_empty()).then_some(nom)
}

// ── Écriture des notes ───────────────────────────────────────────────────────

/// Fusionne des souvenirs dans une note de Jimy (créée au besoin).
async fn merge_note(root: &Path, llm: &LlmClient, model: &str, target: &Target, items: &[Item]) -> Result<()> {
    let chemin = vault::join_folder(root, &target.relative);
    let existant = std::fs::read_to_string(&chemin).ok();
    let (front, ancien) = existant.as_deref().map(split_frontmatter).unwrap_or(("", ""));
    // Jamais de réécriture d'une note qui n'est pas de Jimy.
    if existant.is_some() && front_value(front, "source").as_deref() != Some("jimy") {
        return Err(Error::Tool(format!("{} n'est pas une note de Jimy", target.relative)));
    }
    let nouveaux = items
        .iter()
        .map(|i| format!("- ({}) [{}] {}", i.date, i.kind, i.text))
        .collect::<Vec<_>>()
        .join("\n");
    let mut demande = format!(
        "Note : {}\nTitre : {}\n\nNote actuelle :\n{}\n\nNouveaux éléments à intégrer :\n{nouveaux}",
        target.relative,
        target.title,
        if ancien.trim().is_empty() { "(note neuve)" } else { ancien.trim() }
    );
    if ancien.trim().is_empty() {
        if let Some(modele) = template_body(root, target) {
            demande.push_str(&format!("\n\nStructure de départ (modèle du vault) :\n{modele}"));
        }
    }
    let voisines = sibling_notes(&chemin);
    if !voisines.is_empty() {
        demande.push_str(&format!(
            "\n\nNotes de l'utilisateur dans ce dossier (à lier avec [[Nom]] si utile) : {}",
            voisines.join(", ")
        ));
    }
    let messages = vec![Message::system(FUSION), Message::user(demande)];
    let mut corps = None;
    for attempt in 1..=2 {
        match llm.complete(model, &messages, MAX_TOKENS, Some(0.0)).await {
            Ok(raw) => {
                let propre = clean_markdown(&raw);
                if acceptable(ancien, &propre) {
                    // Un lien vers une note absente du vault redevient du
                    // texte (observé : [[Judge]], [[Jarvis Config]] inventés).
                    corps = Some(strip_unknown_links(&propre, &vault::note_names(root)));
                    break;
                }
                log::warn!(
                    "[vault] fusion rejetée pour {} (tentative {attempt}) : {} caractères contre {}",
                    target.relative,
                    propre.chars().count(),
                    ancien.trim().chars().count()
                );
            }
            Err(error) => log::warn!("[vault] fusion en échec (tentative {attempt}) : {error}"),
        }
        if attempt < 2 {
            tokio::time::sleep(SHORT_RETRY).await;
        }
    }
    let corps = corps.ok_or_else(|| Error::Tool("fusion impossible".into()))?;

    if let Some(ancienne) = &existant {
        backup(root, &target.relative, ancienne)?;
    }
    let jour = today();
    let created = front_value(front, "created").unwrap_or_else(|| jour.clone());
    let note = format!(
        "---\ntitle: \"{}\"\ntype: {}\nsource: jimy\ncreated: {created}\nupdated: {jour}\ntags: [jimy, {}]\n---\n\n{}\n",
        target.title.replace('"', "'"),
        target.dest,
        target.dest,
        corps.trim()
    );
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&chemin, note)?;
    Ok(())
}

/// Ajoute des épisodes au journal mensuel de Jimy, une section par jour,
/// sans appel au modèle et sans répéter une ligne déjà présente.
fn append_journal(root: &Path, target: &Target, items: &[Item]) -> Result<()> {
    let chemin = vault::join_folder(root, &target.relative);
    let mut contenu = std::fs::read_to_string(&chemin).unwrap_or_else(|_| {
        format!(
            "---\ntitle: \"{t}\"\ntype: journal\nsource: jimy\ncreated: {d}\ntags: [jimy, journal]\n---\n\n# {t}\n\n\
             Épisodes retenus par Jimy, jour par jour.\n",
            t = target.title,
            d = today()
        )
    });
    let mut par_jour: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in items {
        let jour = if item.date.is_empty() { today() } else { item.date.clone() };
        let ligne = format!("- {}", item.text);
        if !contenu.lines().any(|l| l.trim() == ligne) {
            par_jour.entry(jour).or_default().push(ligne);
        }
    }
    for (jour, lignes) in par_jour {
        contenu = insert_under_heading(&contenu, &format!("## {jour}"), &lignes);
    }
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&chemin, contenu)?;
    Ok(())
}

/// Insère des lignes à la fin de la section `heading` (créée au besoin).
fn insert_under_heading(content: &str, heading: &str, lines: &[String]) -> String {
    let mut lignes: Vec<String> = content.lines().map(String::from).collect();
    match lignes.iter().position(|l| l.trim() == heading) {
        Some(i) => {
            let fin = lignes[i + 1..]
                .iter()
                .position(|l| l.starts_with("## "))
                .map(|p| i + 1 + p)
                .unwrap_or(lignes.len());
            // Avant les lignes vides qui séparent de la section suivante.
            let mut pos = fin;
            while pos > i + 1 && lignes[pos - 1].trim().is_empty() {
                pos -= 1;
            }
            for (k, ligne) in lines.iter().enumerate() {
                lignes.insert(pos + k, ligne.clone());
            }
        }
        None => {
            if lignes.last().is_some_and(|l| !l.trim().is_empty()) {
                lignes.push(String::new());
            }
            lignes.push(heading.to_string());
            lignes.extend(lines.iter().cloned());
        }
    }
    let mut sortie = lignes.join("\n");
    sortie.push('\n');
    sortie
}

/// Corps du modèle du vault (`_SYSTEM/Templates/<tpl>`), variables remplies.
fn template_body(root: &Path, target: &Target) -> Option<String> {
    let brut = std::fs::read_to_string(root.join("_SYSTEM").join("Templates").join(target.template?)).ok()?;
    let (_, corps) = split_frontmatter(&brut);
    Some(
        corps
            .replace("{{title}}", &target.title)
            .replace("{{date:YYYY-MM-DD}}", &today())
            .trim()
            .to_string(),
    )
}

/// Notes de l'utilisateur dans le dossier de la note (noms sans extension),
/// pour que la fusion puisse les lier.
fn sibling_notes(chemin: &Path) -> Vec<String> {
    let Some(dossier) = chemin.parent() else {
        return Vec::new();
    };
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return Vec::new();
    };
    let mut noms: Vec<String> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .filter(|n| !n.starts_with(OWN_NOTE_PREFIX))
        .collect();
    noms.sort();
    noms.truncate(20);
    noms
}

/// Réponse du modèle → Markdown nu (bloc de code et frontmatter retirés).
fn clean_markdown(raw: &str) -> String {
    let mut texte = raw.trim();
    if let Some(reste) = texte.strip_prefix("```") {
        texte = reste.split_once('\n').map(|(_, r)| r).unwrap_or("");
        texte = texte.trim_end().strip_suffix("```").unwrap_or(texte);
    }
    let (_, corps) = split_frontmatter(texte.trim());
    corps.trim().to_string()
}

/// Remplace `[[Lien]]` par son texte quand aucune note du vault ne porte ce
/// nom (`known` : noms de notes en minuscules, sans extension). Les formes
/// `[[dossier/Nom.md]]`, `[[Nom|alias]]` et `[[Nom#section]]` sont comprises.
fn strip_unknown_links(texte: &str, known: &std::collections::HashSet<String>) -> String {
    let mut sortie = String::new();
    let mut reste = texte;
    while let Some(debut) = reste.find("[[") {
        let Some(fin) = reste[debut + 2..].find("]]").map(|f| debut + 2 + f) else {
            break;
        };
        sortie.push_str(&reste[..debut]);
        let interieur = &reste[debut + 2..fin];
        let (cible, alias) = interieur.split_once('|').unwrap_or((interieur, ""));
        let nom = cible.split('#').next().unwrap_or("").rsplit(['/', '\\']).next().unwrap_or("");
        let nom = nom.trim().trim_end_matches(".md").to_lowercase();
        if known.contains(&nom) {
            sortie.push_str(&reste[debut..fin + 2]);
        } else {
            let texte = if alias.trim().is_empty() { cible.trim().trim_end_matches(".md") } else { alias.trim() };
            sortie.push_str(texte);
        }
        reste = &reste[fin + 2..];
    }
    sortie.push_str(reste);
    sortie
}

/// Bruit des tests d'interface (`scripts/test-ui.ps1` fait écrire Jimy dans
/// `data/tests-ui`) : jamais un souvenir. Observé au premier tri réel, une
/// dizaine de lignes « test travaux » dans le journal.
pub fn is_test_noise(texte: &str) -> bool {
    let bas = texte.to_lowercase();
    ["tests-ui", "reussite-", "réussite-", "test travaux", "fini-fond"]
        .iter()
        .any(|marque| bas.contains(marque))
}

/// Une fusion est acceptable si elle a un titre et ne fait pas fondre la note.
fn acceptable(ancien: &str, nouveau: &str) -> bool {
    let taille = |s: &str| s.trim().chars().count() as f32;
    nouveau.trim_start().starts_with('#') && taille(nouveau) >= taille(ancien) * MIN_KEEP_RATIO
}

/// Copie la note avant réécriture dans `.jimy-historique` (caché).
fn backup(root: &Path, relative: &str, contenu: &str) -> Result<()> {
    let dossier = root.join(HISTORY_DIR);
    std::fs::create_dir_all(&dossier)?;
    let nom = format!("{}.{}.md", relative.trim_end_matches(".md").replace(['/', '\\'], "__"), vault::horodatage_local());
    std::fs::write(dossier.join(nom), contenu)?;
    Ok(())
}

// ── Inbox ────────────────────────────────────────────────────────────────────

/// Fichiers Markdown de l'inbox (premier niveau, hors dossiers cachés).
fn inbox_files(inbox: &Path) -> Vec<PathBuf> {
    let Ok(entrees) = std::fs::read_dir(inbox) else {
        return Vec::new();
    };
    let mut fichiers: Vec<PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")))
        .collect();
    fichiers.sort();
    fichiers
}

/// Souvenirs d'un fichier de l'inbox : les puces d'un fichier du jour, ou le
/// corps d'une ancienne note (un souvenir par fichier, avant le 9 octobre).
pub fn items_of(contenu: &str) -> Vec<Item> {
    let (front, corps) = split_frontmatter(contenu);
    let date = front_value(front, "date").unwrap_or_default();
    if front_value(front, "type").as_deref() == Some("inbox") {
        return corps
            .lines()
            .filter_map(vault::parse_capture)
            .map(|(kind, text)| Item {
                kind,
                text,
                date: date.clone(),
            })
            .collect();
    }
    let texte = corps
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if texte.is_empty() {
        return Vec::new();
    }
    vec![Item {
        kind: front_value(front, "type").unwrap_or_else(|| "semantic".into()),
        text: texte,
        date,
    }]
}

/// Remet dans l'inbox les fichiers d'un tri interrompu ou abandonné. Si un
/// fichier du même nom y a été recréé entre-temps (captures du jour), ses
/// puces sont ajoutées à la suite. À appeler sous `inbox_lock`.
fn restore_staging(inbox: &Path, staging: &Path) -> Result<()> {
    for fichier in inbox_files(staging) {
        let cible = inbox.join(fichier.file_name().unwrap_or_default());
        if cible.exists() {
            let mut contenu = read(&cible);
            if !contenu.ends_with('\n') {
                contenu.push('\n');
            }
            for ligne in read(&fichier).lines().filter(|l| vault::parse_capture(l).is_some()) {
                contenu.push_str(ligne);
                contenu.push('\n');
            }
            std::fs::write(&cible, contenu)?;
            std::fs::remove_file(&fichier)?;
        } else {
            std::fs::rename(&fichier, &cible)?;
        }
    }
    let _ = std::fs::remove_dir(staging);
    Ok(())
}

/// Déplace les fichiers triés dans l'archive de l'inbox, sans jamais
/// écraser un fichier déjà archivé.
fn archive(root: &Path, fichiers: &[PathBuf]) -> Result<()> {
    let dossier = vault::join_folder(root, INBOX_ARCHIVE);
    std::fs::create_dir_all(&dossier)?;
    for fichier in fichiers {
        let nom = stem(fichier);
        let mut cible = dossier.join(format!("{nom}.md"));
        let mut n = 2;
        while cible.exists() {
            cible = dossier.join(format!("{nom}-{n}.md"));
            n += 1;
        }
        std::fs::rename(fichier, &cible)?;
    }
    Ok(())
}

// ── Aides ────────────────────────────────────────────────────────────────────

/// Sépare le frontmatter YAML (`---` … `---`) du corps.
fn split_frontmatter(contenu: &str) -> (&str, &str) {
    let Some(reste) = contenu.strip_prefix("---") else {
        return ("", contenu);
    };
    let Some(reste) = reste.strip_prefix('\n').or_else(|| reste.strip_prefix("\r\n")) else {
        return ("", contenu);
    };
    match reste.find("\n---") {
        Some(fin) => {
            let apres = &reste[fin + 4..];
            let corps = apres.split_once('\n').map(|(_, c)| c).unwrap_or("");
            (&reste[..fin], corps)
        }
        None => ("", contenu),
    }
}

/// Valeur d'une clé du frontmatter (`clé: valeur`, guillemets retirés).
fn front_value(front: &str, key: &str) -> Option<String> {
    front.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        let v = v.trim().trim_matches('"').trim();
        (k.trim() == key && !v.is_empty()).then(|| v.to_string())
    })
}

fn subdirs(dossier: &Path) -> Vec<String> {
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return Vec::new();
    };
    let mut noms: Vec<String> = entrees
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| !n.starts_with('.'))
        .collect();
    noms.sort();
    noms
}

fn read(chemin: &Path) -> String {
    std::fs::read_to_string(chemin).unwrap_or_default()
}

fn stem(chemin: &Path) -> String {
    chemin.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string()
}

/// `AAAA-MM-JJ`, heure de Paris.
fn today() -> String {
    vault::horodatage_local().chars().take(10).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_deux_formats_de_l_inbox_sont_lus() {
        let du_jour = "---\ntype: inbox\nsource: jimy\ndate: 2026-10-09\n---\n\n# Captures du 2026-10-09\n\n\
                       Souvenirs à trier : Jimy les range dans ses notes du vault (PARA).\n\n\
                       - 10:02 [semantic] L'utilisateur préfère les réponses courtes.\n\
                       - 11:15 [procedural] Compiler avec build.ps1.\n";
        let items = items_of(du_jour);
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[1].kind, "procedural");
        assert_eq!(items[1].date, "2026-10-09");
        // Ancienne note : un souvenir par fichier.
        let ancienne = "---\ntype: lesson\nsource: jimmy\ndate: 2026-10-05\n---\n\nAvant de se connecter, vérifier la session.\n";
        assert_eq!(
            items_of(ancienne),
            vec![Item {
                kind: "lesson".into(),
                text: "Avant de se connecter, vérifier la session.".into(),
                date: "2026-10-05".into()
            }]
        );
    }

    #[test]
    fn un_dossier_existant_est_repris_sinon_nomme_comme_le_vault() {
        let existants = vec!["CRM_Ultimate".to_string(), "Jimmy_Agent".to_string()];
        assert_eq!(folder_name("crm ultimate", &existants).as_deref(), Some("CRM_Ultimate"));
        assert_eq!(folder_name("Jimmy-Agent", &existants).as_deref(), Some("Jimmy_Agent"));
        assert_eq!(folder_name("École Été 2026", &existants).as_deref(), Some("Ecole_Ete_2026"));
        assert_eq!(folder_name("../x", &existants).as_deref(), Some("x"));
        assert_eq!(folder_name(" / ", &existants), None);
        // Premier tri réel : le dépôt « jimmy-agent-personnel » créait un
        // doublon du dossier Jimmy_Agent.
        assert_eq!(folder_name("JimmyAgentPersonnel", &existants).as_deref(), Some("Jimmy_Agent"));
        // Un nom court ne capte pas tout ce qui commence pareil.
        let courts = vec!["IA".to_string()];
        assert_eq!(folder_name("IAM_Securite", &courts).as_deref(), Some("IAM_Securite"));
    }

    #[test]
    fn les_liens_vers_des_notes_absentes_redeviennent_du_texte() {
        let known: std::collections::HashSet<String> = ["readme", "handoff"].iter().map(|s| s.to_string()).collect();
        let texte = "Voir [[README]], [[1_Projets/X/handoff.md|le handoff]], [[Judge]] et [[Jarvis Config|la config]].";
        assert_eq!(
            strip_unknown_links(texte, &known),
            "Voir [[README]], [[1_Projets/X/handoff.md|le handoff]], Judge et la config."
        );
        assert_eq!(strip_unknown_links("[[ouvert sans fin", &known), "[[ouvert sans fin");
    }

    #[test]
    fn le_bruit_des_tests_d_interface_est_ecarte() {
        assert!(is_test_noise("Jimmy a écrit le fichier tests-ui/reussite-9641.txt contenant « test travaux »."));
        assert!(is_test_noise("Il a écrit la ligne « test travaux » dans reussite-47.txt."));
        assert!(!is_test_noise("La suite complète est passée au vert : 381 tests."));
    }

    #[test]
    fn chaque_destination_a_sa_note() {
        let structure = Structure {
            projets: vec!["CRM_Ultimate".into()],
            ..Default::default()
        };
        let item = Item {
            kind: "semantic".into(),
            text: "x".into(),
            date: "2026-10-09".into(),
        };
        let cible = |dest: &str, dossier: &str| {
            target_for(&Classed { n: 1, dest: dest.into(), dossier: dossier.into() }, &item, &structure)
                .map(|t| t.relative)
        };
        assert_eq!(cible("projet", "crm ultimate").as_deref(), Some("1_Projets/CRM_Ultimate/Jimy_CRM_Ultimate.md"));
        assert_eq!(cible("ressource", "Serveurs").as_deref(), Some("3_Ressources/Serveurs/Jimy_Serveurs.md"));
        assert_eq!(cible("profil", "").as_deref(), Some("_SYSTEM/Jimy_Memory/Profil.md"));
        assert_eq!(cible("journal", "").as_deref(), Some("_SYSTEM/Journal_Agent/Jimy_2026-10.md"));
        // Projet sans nom de dossier utilisable : journal, pas de dossier vide.
        assert_eq!(cible("projet", "").as_deref(), Some("_SYSTEM/Journal_Agent/Jimy_2026-10.md"));
        assert_eq!(cible("ignorer", ""), None);
        for nom in ["1_Projets/CRM_Ultimate/Jimy_CRM_Ultimate.md", "_SYSTEM/Jimy_Memory/Profil.md", "_SYSTEM/Journal_Agent/Jimy_2026-10.md"] {
            assert!(vault::is_own_note(nom, "0_Inbox/Jimmy"), "{nom} doit être une note de Jimy");
        }
    }

    #[test]
    fn le_classement_tolere_le_texte_autour_et_les_oublis() {
        let raw = "Voici :\n```json\n[{\"n\":1,\"dest\":\"profil\"},{\"n\":3,\"dest\":\"projet\",\"dossier\":\"X\"},{\"n\":9,\"dest\":\"profil\"}]\n```";
        let verdicts = parse_classes(raw, 3).expect("lisible");
        assert_eq!(verdicts[0].as_ref().map(|c| c.dest.as_str()), Some("profil"));
        assert!(verdicts[1].is_none(), "oublié → None");
        assert_eq!(verdicts[2].as_ref().map(|c| c.dossier.as_str()), Some("X"));
        assert!(parse_classes("je ne sais pas", 3).is_none());
    }

    #[test]
    fn une_fusion_qui_fait_fondre_la_note_est_rejetee() {
        let ancien = format!("# Note\n\n{}", "contenu utile ".repeat(50));
        assert!(!acceptable(&ancien, "# Note\n\ncourt"));
        assert!(!acceptable("", "pas de titre"));
        assert!(acceptable(&ancien, &format!("{ancien}\n\n## Ajout\n\nnouveau")));
        assert_eq!(clean_markdown("```markdown\n---\ntitle: x\n---\n# T\n\ncorps\n```"), "# T\n\ncorps");
    }

    #[test]
    fn le_journal_range_par_jour_sans_doublon() {
        let base = "---\ntype: journal\n---\n\n# Journal\n\n## 2026-10-08\n- a\n\n## 2026-10-09\n- b\n";
        let sortie = insert_under_heading(base, "## 2026-10-08", &["- c".into()]);
        assert!(sortie.contains("## 2026-10-08\n- a\n- c\n\n## 2026-10-09"), "{sortie}");
        let sortie = insert_under_heading(&sortie, "## 2026-10-10", &["- d".into()]);
        assert!(sortie.ends_with("## 2026-10-10\n- d\n"), "{sortie}");
    }

    /// Mise de côté puis restauration : les captures arrivées pendant le tri
    /// (même nom de fichier) ne sont pas écrasées.
    #[test]
    fn un_tri_abandonne_rend_toutes_les_captures() {
        let inbox = std::env::temp_dir().join(format!("jimmy-tri-{}", std::process::id()));
        let staging = inbox.join(STAGING);
        std::fs::create_dir_all(&staging).unwrap();
        let entete = "---\ntype: inbox\nsource: jimy\ndate: 2026-10-09\n---\n\n";
        std::fs::write(staging.join("2026-10-09.md"), format!("{entete}- 10:00 [semantic] avant le tri\n")).unwrap();
        std::fs::write(staging.join("2026-10-05T10-00-00-ancienne.md"), "ancienne note").unwrap();
        std::fs::write(inbox.join("2026-10-09.md"), format!("{entete}- 10:05 [semantic] pendant le tri\n")).unwrap();
        restore_staging(&inbox, &staging).unwrap();
        let du_jour = std::fs::read_to_string(inbox.join("2026-10-09.md")).unwrap();
        assert_eq!(items_of(&du_jour).len(), 2, "{du_jour}");
        assert!(inbox.join("2026-10-05T10-00-00-ancienne.md").is_file());
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&inbox);
    }
}
