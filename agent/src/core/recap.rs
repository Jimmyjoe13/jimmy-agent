//! Carnet de bord du contexte : résumé glissant et chantiers ouverts.
//!
//! Deux manques réglés ensemble, par **un seul** appel au modèle, en tâche de
//! fond après la réponse (jamais sur le chemin de la réponse) :
//!
//! * **résumé glissant** (lot 2) : l'historique renvoyé au modèle s'arrête aux
//!   [`WINDOW`] derniers messages ; avant, tout ce qui en sortait disparaissait.
//!   Les messages sortis sont désormais fondus dans un résumé par
//!   conversation, injecté en tête (« Plus tôt dans cette conversation ») ;
//! * **chantiers** (lot 4) : ce qui passait d'une conversation à l'autre,
//!   c'étaient des souvenirs isolés, jamais « où on en est ». Le même appel
//!   tient une liste de chantiers (ouvert, en attente de l'utilisateur, fini)
//!   avec leur prochaine étape, montrée dans **toute** conversation.
//!
//! L'appel n'a lieu qu'après [`RECAP_EVERY`] nouveaux messages (deux
//! échanges) : une conversation d'une question ne coûte rien. Réponse
//! illisible : rien ne change, le prochain tour réessaie.

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

use crate::config::Settings;
use crate::core::history::History;
use crate::core::types::{Message, Role};
use crate::error::Result;
use crate::providers::llm::LlmClient;

/// Messages de conversation renvoyés tels quels au modèle (`History::conversation`).
pub const WINDOW: usize = 20;
/// Nouveaux messages qui déclenchent une mise à jour du carnet.
pub const RECAP_EVERY: usize = 4;
/// Plafonds d'un seul appel : une vieille conversation de la vraie base
/// (des centaines de messages) se fond en plusieurs tours, pas d'un bloc.
const FOLD_MAX: usize = 40;
const RECENT_MAX: usize = 12;
/// Taille maximale du résumé injecté.
pub const SUMMARY_MAX_CHARS: usize = 1500;
/// Chantiers montrés, et taille maximale de leurs lignes.
pub const CHANTIERS_SHOWN: usize = 5;
pub const CHANTIERS_MAX_CHARS: usize = 600;
/// Un chantier qui n'a pas bougé depuis ce délai n'est plus montré (« en
/// sommeil ») : audit du 21 octobre, 17 chantiers figés depuis plus de 7 jours
/// encombraient le prompt.
const STALE_DAYS: i64 = 14;
/// Nouveaux chantiers acceptés par mise à jour (garde-fou contre un modèle
/// qui transformerait chaque phrase en chantier).
const MAX_NEW: usize = 3;
/// Un message relu par le modèle est coupé à cette taille.
const MESSAGE_CHARS: usize = 600;
/// Le raisonnement caché compte dans `max_tokens` (piège 70).
const MAX_TOKENS: u32 = 4000;

const RECAPPER: &str = r#"Tu tiens le carnet de bord de Jimy, l'assistant personnel de l'utilisateur. On te donne le résumé actuel du début d'une conversation (peut être vide), des messages anciens à y fondre (peut être vide), les derniers échanges, et les chantiers déjà suivis.

Réponds UNIQUEMENT par un objet JSON, sans texte autour :
{"titre": "…", "resume": "…", "chantiers": [{"id": "c1", "titre": "…", "statut": "ouvert", "prochaine": "…"}]}

"titre" : nom court de la conversation (3 à 7 mots) qui dit de quoi elle parle surtout, comme un titre de dossier (« Relance des leads Maxence »). Sans date, sans « conversation », sans guillemets. Vide si rien de clair.

"resume" : le résumé actuel complété par les messages anciens (s'il n'y en a pas, recopie le résumé actuel tel quel). Moins de 1200 caractères, en français, des faits concrets : sujets, décisions, fichiers, résultats, demandes restées ouvertes. Pas de politesse, pas de méta-commentaire.

"chantiers" : seulement ceux que ces échanges touchent. Un chantier est un travail qui s'étale dans le temps (projet, tâche à reprendre, question en suspens) — pas une question ponctuelle déjà réglée, pas une conversation banale.
- Même chantier qu'un chantier suivi : reprends son id. Nouveau : "id": null.
- "statut" : "attente" si Jimy attend une réponse ou une action de l'utilisateur ; "ouvert" s'il reste du travail ; "fini" s'il est terminé ou abandonné.
- "titre" : 3 à 8 mots. "prochaine" : la prochaine étape concrète en une phrase (vide si fini).
- Liste vide si rien ne mérite d'être suivi."#;

/// État du carnet pour une conversation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recap {
    pub summary: String,
    /// Messages de conversation déjà fondus dans le résumé.
    pub folded: usize,
    /// Messages de conversation déjà relus (chantiers).
    pub seen: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chantier {
    pub id: String,
    pub title: String,
    pub status: String,
    pub next_step: String,
    pub session_id: Option<String>,
    pub updated_at: String,
}

/// Travail à faire pour une mise à jour : messages à fondre, messages récents.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub fold: std::ops::Range<usize>,
    pub recent: std::ops::Range<usize>,
}

/// Faut-il mettre le carnet à jour, et avec quoi ? `total` = messages de
/// conversation de la session. Pur, pour les tests.
pub fn plan(total: usize, recap: &Recap) -> Option<Plan> {
    let seen = recap.seen.min(total);
    if total - seen < RECAP_EVERY {
        return None;
    }
    let fold_end = total.saturating_sub(WINDOW);
    let fold_start = recap.folded.min(fold_end);
    Some(Plan {
        fold: fold_start..fold_end.min(fold_start + FOLD_MAX),
        recent: seen.max(total.saturating_sub(RECENT_MAX))..total,
    })
}

#[derive(Debug, Deserialize)]
struct Output {
    /// Titre court de la conversation (absent des anciennes réponses).
    #[serde(default)]
    titre: String,
    #[serde(default)]
    resume: String,
    #[serde(default)]
    chantiers: Vec<Update>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Update {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    titre: String,
    #[serde(default)]
    statut: String,
    #[serde(default)]
    prochaine: String,
}

fn parse(raw: &str) -> Option<Output> {
    let block = crate::memory::learn::extract_json_block(raw)?;
    serde_json::from_str(&block).ok()
}

/// Statut normalisé (« en attente », « Attente » → `attente`).
fn status_of(raw: &str) -> &'static str {
    let s = raw.to_lowercase();
    if s.contains("attente") {
        "attente"
    } else if s.contains("fini") || s.contains("termin") || s.contains("abandon") {
        "fini"
    } else {
        "ouvert"
    }
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

impl History {
    /// Carnet de la conversation (vide si jamais mis à jour).
    pub fn recap(&self, session_id: &str) -> Result<Recap> {
        let conn = self.db.lock().unwrap();
        let row = conn
            .conn()
            .query_row(
                "SELECT summary, folded, seen FROM session_recaps WHERE session_id = ?1",
                params![session_id],
                |r| Ok(Recap { summary: r.get(0)?, folded: r.get::<_, i64>(1)? as usize, seen: r.get::<_, i64>(2)? as usize }),
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    fn save_recap(&self, session_id: &str, recap: &Recap) -> Result<()> {
        self.db.lock().unwrap().conn().execute(
            "INSERT INTO session_recaps (session_id, summary, folded, seen, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id) DO UPDATE SET summary = ?2, folded = ?3, seen = ?4, updated_at = ?5",
            params![session_id, recap.summary, recap.folded as i64, recap.seen as i64, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Chantiers non finis et récents, du plus récent au plus ancien, tous
    /// projets confondus.
    pub fn chantiers(&self, limit: usize) -> Result<Vec<Chantier>> {
        self.chantiers_for(None, limit)
    }

    /// Chantiers vus depuis une conversation rattachée à `project` : ceux du
    /// projet d'abord, puis ceux sans projet ; jamais ceux d'un autre projet.
    /// Sans projet : tous.
    pub fn chantiers_for(&self, project: Option<&str>, limit: usize) -> Result<Vec<Chantier>> {
        let since = (Utc::now() - chrono::Duration::days(STALE_DAYS)).to_rfc3339();
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT id, title, status, next_step, session_id, updated_at FROM chantiers
             WHERE status != 'fini' AND updated_at >= ?1
               AND (?3 IS NULL OR project IS NULL OR project = '' OR project = ?3)
             ORDER BY (project = ?3) DESC, updated_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit as i64, project], |r| {
            Ok(Chantier {
                id: r.get(0)?,
                title: r.get(1)?,
                status: r.get(2)?,
                next_step: r.get(3)?,
                session_id: r.get(4)?,
                updated_at: r.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Applique les chantiers rendus par le modèle. `known` relie les alias
    /// montrés au modèle (`c1`, `c2`…) aux identifiants réels.
    fn apply_chantiers(&self, session_id: &str, project: Option<&str>, known: &[(String, Chantier)], updates: Vec<Update>) -> Result<usize> {
        let now = Utc::now().to_rfc3339();
        let conn = self.db.lock().unwrap();
        let mut changed = 0usize;
        let mut created = 0usize;
        for update in updates {
            let title = clip(&update.titre, 80);
            let status = status_of(&update.statut);
            let next = if status == "fini" { String::new() } else { clip(&update.prochaine, 160) };
            let existing = update.id.as_deref().and_then(|alias| known.iter().find(|(a, _)| a == alias));
            match existing {
                Some((_, chantier)) => {
                    let title = if title.is_empty() { chantier.title.clone() } else { title };
                    conn.conn().execute(
                        "UPDATE chantiers SET title = ?2, status = ?3, next_step = ?4, session_id = ?5, updated_at = ?6 WHERE id = ?1",
                        params![chantier.id, title, status, next, session_id, now],
                    )?;
                    changed += 1;
                }
                // Nouveau chantier : jamais déjà fini, jamais sans titre,
                // jamais né d'un test d'interface (« garde-3344 »).
                None if status != "fini"
                    && !title.is_empty()
                    && created < MAX_NEW
                    && !crate::memory::sort::is_test_noise(&format!("{title} {next}")) =>
                {
                    conn.conn().execute(
                        "INSERT INTO chantiers (id, title, status, next_step, project, session_id, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![uuid::Uuid::new_v4().to_string(), title, status, next, project, session_id, now],
                    )?;
                    created += 1;
                    changed += 1;
                }
                None => {}
            }
        }
        Ok(changed)
    }
}

/// Bloc « Plus tôt dans cette conversation » (vide : rien).
pub fn summary_block(recap: &Recap) -> Option<String> {
    let summary = recap.summary.trim();
    if summary.is_empty() {
        return None;
    }
    let summary: String = if summary.chars().count() > SUMMARY_MAX_CHARS {
        format!("{}…", summary.chars().take(SUMMARY_MAX_CHARS - 1).collect::<String>())
    } else {
        summary.to_string()
    };
    Some(format!(
        "## Plus tôt dans cette conversation\nRésumé des échanges sortis de l'historique ci-dessous :\n{summary}"
    ))
}

/// Bloc « Chantiers ouverts », borné à [`CHANTIERS_MAX_CHARS`] de lignes.
pub fn chantiers_block(chantiers: &[Chantier], session_id: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut used = 0usize;
    for chantier in chantiers.iter().take(CHANTIERS_SHOWN) {
        let status = if chantier.status == "attente" { "en attente de l'utilisateur" } else { "en cours" };
        let place = if chantier.session_id.as_deref() == Some(session_id) {
            "cette conversation".to_string()
        } else {
            ago(&chantier.updated_at)
        };
        let next = if chantier.next_step.is_empty() { String::new() } else { format!(" — prochaine étape : {}", chantier.next_step) };
        let line = format!("- {} [{status}, {place}]{next}", chantier.title);
        let size = line.chars().count() + 1;
        if used + size > CHANTIERS_MAX_CHARS {
            break;
        }
        used += size;
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "## Chantiers ouverts\nTravaux suivis d'une conversation à l'autre. Si l'utilisateur en reprend un, repars de là ; \
         ne les évoque pas sans raison.\n{}",
        lines.join("\n")
    ))
}

/// « aujourd'hui », « hier », « il y a 3 j ».
fn ago(rfc3339: &str) -> String {
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(rfc3339) else { return "récemment".into() };
    match (Utc::now() - at.with_timezone(&Utc)).num_days() {
        0 => "aujourd'hui".into(),
        1 => "hier".into(),
        n => format!("il y a {n} j"),
    }
}

/// Une conversation sous forme de lignes « Utilisateur : … » / « Jimy : … ».
fn transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| {
            let who = if m.role == Role::User { "Utilisateur" } else { "Jimy" };
            format!("{who} : {}", clip(&m.content, MESSAGE_CHARS))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Carnet après une mise à jour. Le résumé ne change que s'il y avait des
/// messages à fondre : sinon le modèle résume des messages encore dans
/// l'historique (observé au premier essai réel : un « résumé » qui doublait
/// la fenêtre). Un résumé vide alors qu'il fallait fondre : on garde l'ancien
/// et rien n'est marqué fondu (réessai au prochain tour).
fn next_recap(recap: &Recap, plan: &Plan, resume: &str) -> Recap {
    let resume = resume.trim();
    if plan.fold.is_empty() || resume.is_empty() {
        return Recap { seen: plan.recent.end, ..recap.clone() };
    }
    Recap { summary: resume.to_string(), folded: plan.fold.end.max(recap.folded), seen: plan.recent.end }
}

/// Un seul carnet mis à jour à la fois : deux tours qui finissent ensemble
/// (Chat + tâche de fond) ne doivent pas créer deux fois le même chantier.
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Met le carnet de la conversation à jour si c'est le moment (voir
/// [`plan`]). Appelé en tâche de fond après chaque réponse.
pub async fn update(llm: &LlmClient, settings: &Settings, history: &History, session_id: &str) {
    let _guard = RUNNING.lock().await;
    let Ok(conversation) = history.conversation(session_id, usize::MAX) else { return };
    let Ok(recap) = history.recap(session_id) else { return };
    let Some(plan) = plan(conversation.len(), &recap) else { return };
    // Projet de la conversation : le modèle ne voit (et ne touche) que les
    // chantiers de ce projet et ceux sans projet.
    let project = history.project(session_id).ok().flatten();
    let known: Vec<(String, Chantier)> = history
        .chantiers_for(project.as_deref(), 12)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, c)| (format!("c{}", i + 1), c))
        .collect();
    let suivis = if known.is_empty() {
        "(aucun)".to_string()
    } else {
        known
            .iter()
            .map(|(alias, c)| format!("- {alias} : {} [{}] — prochaine : {}", c.title, c.status, c.next_step))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let fold = transcript(&conversation[plan.fold.clone()]);
    let recent = transcript(&conversation[plan.recent.clone()]);
    let messages = vec![
        Message::system(crate::i18n::with_background_lang(RECAPPER, settings.lang())),
        Message::user(format!(
            "Résumé actuel :\n{}\n\nMessages anciens à fondre :\n{}\n\nDerniers échanges :\n{}\n\nChantiers suivis :\n{}",
            if recap.summary.is_empty() { "(vide)" } else { &recap.summary },
            if fold.is_empty() { "(aucun)".to_string() } else { fold },
            recent,
            suivis
        )),
    ];
    let raw = match llm.complete(settings.memory_model(), &messages, MAX_TOKENS, None).await {
        Ok(raw) => raw,
        Err(error) => {
            log::warn!("[contexte] carnet non mis à jour : {error}");
            return;
        }
    };
    let Some(output) = parse(&raw) else {
        log::warn!("[contexte] carnet illisible : {}", crate::sensitive::mask_text(&raw.chars().take(200).collect::<String>()));
        return;
    };
    let next = next_recap(&recap, &plan, &output.resume);
    // Le carnet d'abord : si la conversation a été supprimée pendant l'appel
    // (clé étrangère), on s'arrête là, sans créer de chantier orphelin.
    if let Err(error) = history.save_recap(session_id, &next) {
        log::info!("[contexte] carnet non écrit (conversation supprimée ?) : {error}");
        return;
    }
    // Titre de la conversation (« Session vocale » ne dit rien) ; jamais
    // par-dessus un titre choisi par l'utilisateur (`History::set_title_auto`).
    if let Err(error) = history.set_title_auto(session_id, &output.titre) {
        log::warn!("[contexte] titre non écrit : {error}");
    }
    let changed = history
        .apply_chantiers(session_id, project.as_deref(), &known, output.chantiers)
        .unwrap_or_else(|error| {
            log::warn!("[contexte] chantiers non écrits : {error}");
            0
        });
    log::info!(
        "[contexte] carnet à jour : {} message(s) fondu(s), résumé {} caractères, {changed} chantier(s) touché(s)",
        plan.fold.len(),
        next.summary.chars().count()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> History {
        History::new(std::sync::Arc::new(std::sync::Mutex::new(crate::db::Db::open_in_memory().unwrap())))
    }

    /// Le défaut d'origine : au-delà de la fenêtre, le début de la
    /// conversation n'arrive plus au modèle — d'où le résumé.
    #[test]
    fn le_debut_d_une_longue_conversation_sort_de_la_fenetre() {
        let h = history();
        let s = h.create_session("t").unwrap();
        for i in 0..15 {
            h.append(&s, &Message::user(format!("question {i}"))).unwrap();
            h.append(&s, &Message::assistant(format!("réponse {i}"))).unwrap();
        }
        let window = h.conversation(&s, WINDOW).unwrap();
        assert!(!window.iter().any(|m| m.content == "question 0"));
        // Le carnet doit fondre les 10 premiers messages, et relire les 12
        // derniers pour les chantiers.
        let plan = plan(30, &h.recap(&s).unwrap()).unwrap();
        assert_eq!(plan.fold, 0..10);
        assert_eq!(plan.recent, 18..30);
    }

    #[test]
    fn le_carnet_attend_deux_echanges_nouveaux() {
        let recap = Recap { summary: "x".into(), folded: 10, seen: 30 };
        assert_eq!(plan(33, &recap), None);
        let p = plan(34, &recap).unwrap();
        assert_eq!(p.fold, 10..14);
        assert_eq!(p.recent, 30..34);
        // Conversation courte : rien à fondre, seulement les chantiers.
        assert_eq!(plan(4, &Recap::default()).unwrap().fold, 0..0);
        // Vieille conversation jamais résumée : fondue par morceaux de 40.
        let old = plan(500, &Recap::default()).unwrap();
        assert_eq!(old.fold, 0..40);
        assert_eq!(old.recent, 488..500);
    }

    #[test]
    fn le_resume_ne_bouge_que_s_il_y_a_des_messages_a_fondre() {
        let avant = Recap { summary: String::new(), folded: 0, seen: 0 };
        // Rien à fondre : le « résumé » inventé par le modèle est ignoré.
        let rien = Plan { fold: 0..0, recent: 0..4 };
        assert_eq!(next_recap(&avant, &rien, "On a parlé de bananes."), Recap { seen: 4, ..avant.clone() });
        // Des messages à fondre : le résumé est pris, les messages marqués.
        let fondre = Plan { fold: 0..6, recent: 14..26 };
        assert_eq!(
            next_recap(&avant, &fondre, " Début : le VPS. "),
            Recap { summary: "Début : le VPS.".into(), folded: 6, seen: 26 }
        );
        // Réponse vide alors qu'il fallait fondre : rien n'est marqué fondu.
        assert_eq!(next_recap(&avant, &fondre, "  ").folded, 0);
    }

    /// Conversation supprimée pendant l'appel : le carnet échoue (clé
    /// étrangère), ce qui doit arrêter la mise à jour avant les chantiers.
    #[test]
    fn le_carnet_d_une_conversation_supprimee_echoue() {
        let h = history();
        let s = h.create_session("t").unwrap();
        h.delete_session(&s).unwrap();
        assert!(h.save_recap(&s, &Recap::default()).is_err());
    }

    #[test]
    fn le_resume_est_injecte_et_borne() {
        assert_eq!(summary_block(&Recap::default()), None);
        let block = summary_block(&Recap { summary: "a".repeat(5000), ..Default::default() }).unwrap();
        assert!(block.starts_with("## Plus tôt dans cette conversation"));
        assert!(block.chars().count() < SUMMARY_MAX_CHARS + 120, "{}", block.chars().count());
    }

    #[test]
    fn les_chantiers_suivent_d_une_conversation_a_l_autre() {
        let h = history();
        let a = h.create_session("a").unwrap();
        let b = h.create_session("b").unwrap();
        let output = parse(
            "```json\n{\"resume\": \"\", \"chantiers\": [\
             {\"id\": null, \"titre\": \"Refonte du site Elu\", \"statut\": \"en attente\", \"prochaine\": \"valider la maquette\"},\
             {\"id\": null, \"titre\": \"Déjà fait\", \"statut\": \"fini\", \"prochaine\": \"\"}]}\n```",
        )
        .unwrap();
        assert_eq!(h.apply_chantiers(&a, Some("C:\\elu"), &[], output.chantiers).unwrap(), 1);
        let list = h.chantiers(5).unwrap();
        assert_eq!(list.len(), 1, "un chantier né fini n'est pas créé");
        assert_eq!(list[0].status, "attente");
        // Vu depuis une autre conversation : montré, avec sa date.
        let block = chantiers_block(&list, &b).unwrap();
        assert!(block.contains("Refonte du site Elu [en attente de l'utilisateur, aujourd'hui]"), "{block}");
        assert!(block.contains("valider la maquette"), "{block}");
        // Le modèle le déclare fini via son alias : il disparaît.
        let known = vec![("c1".to_string(), list[0].clone())];
        let done = vec![Update { id: Some("c1".into()), titre: String::new(), statut: "fini".into(), prochaine: String::new() }];
        h.apply_chantiers(&b, None, &known, done).unwrap();
        assert!(h.chantiers(5).unwrap().is_empty());
    }

    fn nouveau(titre: &str, prochaine: &str) -> Update {
        Update { id: None, titre: titre.into(), statut: "ouvert".into(), prochaine: prochaine.into() }
    }

    /// Audit du 21 octobre : un chantier Emile s'affichait dans une session de
    /// développement de Jimy. Une conversation rattachée à un projet ne voit
    /// que les chantiers de ce projet et ceux sans projet ; sans projet, tout.
    #[test]
    fn les_chantiers_d_un_autre_projet_sont_masques() {
        let h = history();
        let s = h.create_session("s").unwrap();
        h.apply_chantiers(&s, Some("C:\\emile"), &[], vec![nouveau("Campagnes Emile", "relancer")]).unwrap();
        h.apply_chantiers(&s, Some("C:\\jimy"), &[], vec![nouveau("Refonte des sessions", "tester")]).unwrap();
        h.apply_chantiers(&s, None, &[], vec![nouveau("Journal de bord", "clore la journée")]).unwrap();

        let titres = |p: Option<&str>| h.chantiers_for(p, 10).unwrap().into_iter().map(|c| c.title).collect::<Vec<_>>();
        let jimy = titres(Some("C:\\jimy"));
        assert_eq!(jimy[0], "Refonte des sessions", "le projet courant d'abord : {jimy:?}");
        assert!(jimy.contains(&"Journal de bord".to_string()), "{jimy:?}");
        assert!(!jimy.contains(&"Campagnes Emile".to_string()), "{jimy:?}");
        assert_eq!(titres(None).len(), 3);
    }

    /// `test-ui` créait des chantiers (« Création fichier garde-3344 ») :
    /// jamais un chantier.
    #[test]
    fn un_chantier_de_test_n_est_pas_cree() {
        let h = history();
        let s = h.create_session("s").unwrap();
        let bruit = vec![
            nouveau("Création fichier garde-3344", "Obtenir confirmation pour écrire TEST=1"),
            nouveau("Écriture toujours-3850.env", "réécrire"),
        ];
        assert_eq!(h.apply_chantiers(&s, None, &[], bruit).unwrap(), 0);
        assert!(h.chantiers(10).unwrap().is_empty());
    }

    /// Le carnet donne un titre court à la conversation.
    #[test]
    fn le_carnet_rend_un_titre() {
        let output = parse("{\"titre\": \"Relance des leads Maxence\", \"resume\": \"\", \"chantiers\": []}").unwrap();
        assert_eq!(output.titre, "Relance des leads Maxence");
        // Ancien format (sans titre) : toujours lisible.
        assert_eq!(parse("{\"resume\": \"x\", \"chantiers\": []}").unwrap().titre, "");
    }

    #[test]
    fn le_bloc_des_chantiers_est_borne() {
        let list: Vec<Chantier> = (0..10)
            .map(|i| Chantier {
                id: format!("{i}"),
                title: format!("Chantier numéro {i} avec un titre assez long"),
                status: "ouvert".into(),
                next_step: "x".repeat(150),
                session_id: None,
                updated_at: Utc::now().to_rfc3339(),
            })
            .collect();
        let block = chantiers_block(&list, "s").unwrap();
        let lines = block.lines().filter(|l| l.starts_with("- ")).count();
        assert!((1..=CHANTIERS_SHOWN).contains(&lines), "{block}");
        assert!(block.lines().filter(|l| l.starts_with("- ")).map(|l| l.chars().count() + 1).sum::<usize>() <= CHANTIERS_MAX_CHARS);
    }
}
