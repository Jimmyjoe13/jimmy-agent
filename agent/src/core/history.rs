//! Historique des conversations, stocké dans SQLite.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::core::types::{Message, Role};
use crate::db::Db;
use crate::error::Result;

/// Verrou poisoning : on préfère continuer plutôt que planter si un thread
/// panique en cours d'écriture. Le contenu reste cohérent.
pub type Shared<T> = Arc<Mutex<T>>;

pub struct History {
    db: Shared<Db>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
    /// Dossier du projet de la conversation (onglet Chat), s'il y en a un.
    pub project: Option<String>,
}

impl History {
    pub fn new(db: Shared<Db>) -> Self {
        History { db }
    }

    pub fn create_session(&self, title: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.lock().unwrap().conn().execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, title, now],
        )?;
        Ok(id)
    }

    pub fn touch(&self, session_id: &str) -> Result<()> {
        self.db.lock().unwrap().conn().execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![session_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Titre automatique à partir du premier message de la session.
    pub fn auto_title(&self, session_id: &str, from: &str) -> Result<()> {
        let trimmed: String = from.chars().take(60).collect();
        self.db.lock().unwrap().conn().execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1 AND (title = 'Nouvelle session' OR title = '')",
            params![session_id, trimmed],
        )?;
        Ok(())
    }

    pub fn append(&self, session_id: &str, message: &Message) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let tool_calls = message
            .tool_calls
            .as_ref()
            .map(|calls| serde_json::to_string(calls))
            .transpose()?;
        let conn = self.db.lock().unwrap();
        conn.conn().execute(
            "INSERT INTO messages (id, session_id, role, content, tool_name, tool_calls, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                session_id,
                message.role.as_str(),
                message.content,
                message.name,
                tool_calls,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    /// Historique d'une session, dans l'ordre chronologique.
    pub fn messages(&self, session_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT role, content, tool_calls, tool_name FROM messages
             WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let role: String = row.get(0)?;
            let content: String = row.get(1)?;
            let tool_calls: Option<String> = row.get(2)?;
            let tool_name: Option<String> = row.get(3)?;
            Ok((role, content, tool_calls, tool_name))
        })?;

        let mut out: Vec<Message> = Vec::new();
        for row in rows {
            let (role, content, tool_calls, tool_name) = row?;
            out.push(Message {
                role: Role::from_str(&role),
                content,
                tool_calls: tool_calls.and_then(|raw| serde_json::from_str(&raw).ok()),
                tool_call_id: None,
                name: tool_name,
            });
        }
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        Ok(out)
    }

    /// La **conversation** d'une session, prête à être renvoyée au modèle :
    /// les messages de l'utilisateur et les réponses finales de Jimmy, sans les
    /// appels d'outils ni leurs résultats.
    ///
    /// Cause de l'erreur « HTTP 400 invalid request » vécue en usage réel :
    /// l'historique complet était rechargé tel quel, avec deux défauts. Les
    /// messages `tool` perdaient leur `tool_call_id` (jamais stocké), et la
    /// troncature aux 20 derniers messages laissait des résultats d'outils
    /// orphelins en tête de séquence. L'API refuse les deux. Les outils d'un
    /// tour précédent n'ont d'ailleurs aucune utilité : la réponse finale les
    /// résume, et les renvoyer faisait grossir le contexte de 3 k à 15 k jetons.
    pub fn conversation(&self, session_id: &str, limit: usize) -> Result<Vec<Message>> {
        let mut out: Vec<Message> = self
            .messages(session_id, usize::MAX)?
            .into_iter()
            .filter(|m| match m.role {
                Role::User => !m.content.trim().is_empty(),
                Role::Assistant => {
                    m.tool_calls.as_ref().map_or(true, |calls| calls.is_empty()) && !m.content.trim().is_empty()
                }
                _ => false,
            })
            .collect();
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        // La séquence doit commencer par l'utilisateur.
        while out.first().is_some_and(|m| m.role != Role::User) {
            out.remove(0);
        }
        Ok(out)
    }

    /// Résumé compact des outils appelés pendant les `turns` derniers tours :
    /// nom, arguments et début du résultat, une ligne par appel.
    ///
    /// `conversation` ne renvoie jamais les messages d'outils (piège 45 : l'API
    /// refuse une séquence incomplète, et les résultats bruts faisaient passer
    /// le contexte de 3 k à 15 k jetons). Mais sans aucune trace, le modèle
    /// oubliait au tour suivant ce que ses outils avaient trouvé. Ce résumé va
    /// dans le prompt système : aucun risque de séquence invalide, et un coût
    /// borné par `max_chars` (≈ 400 jetons pour 1 500 caractères).
    pub fn tool_digest(&self, session_id: &str, turns: usize, max_chars: usize) -> Result<Option<String>> {
        // Découpage en tours : chaque message de l'utilisateur en ouvre un.
        let mut tours: Vec<(String, Vec<String>)> = Vec::new();
        let mut pending: std::collections::VecDeque<(String, String)> = Default::default();
        for message in self.messages(session_id, usize::MAX)? {
            match message.role {
                Role::User => {
                    tours.push((one_line(&message.content, DIGEST_REQUEST_CHARS), Vec::new()));
                    pending.clear();
                }
                Role::Assistant => {
                    for call in message.tool_calls.unwrap_or_default() {
                        let args = serde_json::to_string(&call.arguments).unwrap_or_default();
                        pending.push_back((call.name, one_line(&args, DIGEST_ARGS_CHARS)));
                    }
                }
                Role::Tool => {
                    // Les résultats sont enregistrés dans l'ordre des appels.
                    let (name, args) = pending
                        .pop_front()
                        .unwrap_or_else(|| (message.name.clone().unwrap_or_default(), String::new()));
                    if let Some((_, lines)) = tours.last_mut() {
                        lines.push(format!("  - {name} {args} → {}", one_line(&message.content, DIGEST_RESULT_CHARS)));
                    }
                }
                _ => {}
            }
        }
        let start = tours.len().saturating_sub(turns);
        // Du plus récent au plus ancien : si le budget est dépassé, ce sont les
        // tours anciens qui sautent.
        let header = "## Travail récent dans cette session\nOutils déjà appelés (résultats abrégés). Appuie-toi dessus pour comprendre la suite de la conversation ; relance un outil seulement si le détail te manque.";
        let mut blocks: Vec<String> = Vec::new();
        let mut used = header.chars().count();
        for (request, lines) in tours[start..].iter().rev().filter(|(_, lines)| !lines.is_empty()) {
            let mut block = format!("- Demande « {request} » :");
            for line in lines.iter().take(DIGEST_CALLS_PER_TURN) {
                block.push('\n');
                block.push_str(line);
            }
            if lines.len() > DIGEST_CALLS_PER_TURN {
                block.push_str(&format!("\n  - … et {} autre(s) appel(s)", lines.len() - DIGEST_CALLS_PER_TURN));
            }
            let size = block.chars().count() + 1;
            if used + size > max_chars {
                break;
            }
            used += size;
            blocks.push(block);
        }
        if blocks.is_empty() {
            return Ok(None);
        }
        blocks.reverse();
        Ok(Some(format!("{header}\n{}", blocks.join("\n"))))
    }

    pub fn sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    (SELECT count(*) FROM messages m WHERE m.session_id = s.id), s.project
             FROM sessions s ORDER BY s.updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row: &Row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
                project: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Rattache (ou détache, `None`) un projet à une conversation : l'agent y
    /// travaille alors (dossier de travail de ses outils).
    pub fn set_project(&self, session_id: &str, project: Option<&str>) -> Result<()> {
        let project = project.map(str::trim).filter(|p| !p.is_empty());
        self.db.lock().unwrap().conn().execute(
            "UPDATE sessions SET project = ?2 WHERE id = ?1",
            params![session_id, project],
        )?;
        Ok(())
    }

    /// Projet d'une conversation.
    pub fn project(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.db.lock().unwrap();
        let project = conn
            .conn()
            .query_row("SELECT project FROM sessions WHERE id = ?1", params![session_id], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten();
        Ok(project)
    }

    /// Projets utilisés récemment, du plus récent au plus ancien, sans doublon.
    pub fn recent_projects(&self, limit: usize) -> Result<Vec<String>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT project FROM sessions WHERE project IS NOT NULL AND project != ''
             GROUP BY project ORDER BY max(updated_at) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.db.lock().unwrap().conn().execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
}

/// Budgets du résumé des outils récents (`History::tool_digest`).
const DIGEST_REQUEST_CHARS: usize = 70;
const DIGEST_ARGS_CHARS: usize = 90;
const DIGEST_RESULT_CHARS: usize = 110;
const DIGEST_CALLS_PER_TURN: usize = 5;

/// Texte sur une ligne (blancs fusionnés), tronqué à `max` caractères.
fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ToolCall;

    fn history() -> History {
        let db = Arc::new(Mutex::new(Db::open_in_memory().expect("db")));
        History::new(db)
    }

    /// Un tour avec outils, tel que l'agent l'enregistre : demande, appel
    /// d'outil, résultats, réponse finale.
    fn tour_avec_outils(h: &History, session: &str, demande: &str, reponse: &str, outils: usize) {
        h.append(session, &Message::user(demande)).unwrap();
        let mut appel = Message::assistant("Je regarde.");
        appel.tool_calls = Some(
            (0..outils)
                .map(|i| ToolCall {
                    id: format!("call_{i}"),
                    name: "list_directory".into(),
                    arguments: serde_json::json!({ "path": "." }),
                })
                .collect(),
        );
        h.append(session, &appel).unwrap();
        for i in 0..outils {
            h.append(session, &Message::tool_result(format!("call_{i}"), "list_directory", "contenu")).unwrap();
        }
        h.append(session, &Message::assistant(reponse)).unwrap();
    }

    /// Cas réel du 4 octobre : au tour suivant, le modèle ne savait plus ce
    /// que ses outils avaient trouvé (« installe-le » → « installer quoi ? »).
    /// Le résumé des outils récents redonne ce contexte, en peu de caractères.
    #[test]
    fn le_resume_des_outils_recents_est_compact_et_borne() {
        let h = history();
        let s = h.create_session("t").unwrap();
        assert!(h.tool_digest(&s, 3, 1500).unwrap().is_none(), "session vide : rien");
        tour_avec_outils(&h, &s, "Tour ancien", "Ok.", 2);
        for i in 0..3 {
            tour_avec_outils(&h, &s, &format!("Inspecte le skill {i}"), "Voilà.", 2);
        }
        // Un résultat d'outil très long ne doit pas passer en entier.
        h.append(&s, &Message::user("Lis le gros fichier")).unwrap();
        let mut appel = Message::assistant("");
        appel.tool_calls = Some(vec![ToolCall {
            id: "c".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "C:\\gros.txt" }),
        }]);
        h.append(&s, &appel).unwrap();
        h.append(&s, &Message::tool_result("c", "read_file", "x".repeat(20_000))).unwrap();
        h.append(&s, &Message::assistant("Fini.")).unwrap();

        let digest = h.tool_digest(&s, 3, 1500).unwrap().expect("résumé");
        assert!(digest.contains("read_file"), "{digest}");
        assert!(digest.contains("gros.txt"), "les arguments sont gardés : {digest}");
        assert!(digest.contains("list_directory"), "{digest}");
        // Seuls les 3 derniers tours : le tour ancien n'y est plus.
        assert!(!digest.contains("Tour ancien"), "{digest}");
        assert!(digest.chars().count() <= 1500, "{} caractères", digest.chars().count());
        assert!(!digest.contains(&"x".repeat(300)), "résultat d'outil non tronqué");
    }

    #[test]
    fn une_conversation_garde_son_projet_et_les_projets_recents_sont_listes() {
        let h = history();
        let a = h.create_session("a").unwrap();
        let b = h.create_session("b").unwrap();
        assert_eq!(h.project(&a).unwrap(), None);
        h.set_project(&a, Some("C:\\Users\\jimmy\\Projet\\alpha")).unwrap();
        h.set_project(&b, Some("C:\\Users\\jimmy\\Projet\\beta")).unwrap();
        h.append(&b, &Message::user("salut")).unwrap();
        assert_eq!(h.project(&a).unwrap().as_deref(), Some("C:\\Users\\jimmy\\Projet\\alpha"));
        let recents = h.recent_projects(10).unwrap();
        assert_eq!(recents.len(), 2);
        assert!(recents[0].ends_with("beta"), "le plus récent d'abord : {recents:?}");
        // Détacher, et un projet vide ne compte pas.
        h.set_project(&a, Some("  ")).unwrap();
        assert_eq!(h.project(&a).unwrap(), None);
        assert_eq!(h.sessions(10).unwrap().iter().find(|s| s.id == b).unwrap().project.as_deref(), Some("C:\\Users\\jimmy\\Projet\\beta"));
    }

    /// La conversation ne contient jamais d'outil : ni appel, ni résultat.
    #[test]
    fn la_conversation_ne_contient_aucun_outil() {
        let h = history();
        let s = h.create_session("t").unwrap();
        tour_avec_outils(&h, &s, "Analyse le dossier", "Voilà.", 3);
        let conv = h.conversation(&s, 20).unwrap();
        assert_eq!(conv.len(), 2, "{conv:?}");
        assert_eq!(conv[0].role, Role::User);
        assert_eq!(conv[1].role, Role::Assistant);
        assert!(conv.iter().all(|m| m.tool_calls.is_none() && m.role != Role::Tool));
    }

    /// Le cas réel : plusieurs tours avec beaucoup d'outils, puis une fenêtre
    /// de 20 messages qui, avant la correction, commençait par des résultats
    /// d'outils orphelins. La séquence doit toujours commencer par l'utilisateur
    /// et alterner sans trou.
    #[test]
    fn la_fenetre_ne_commence_jamais_par_un_orphelin() {
        let h = history();
        let s = h.create_session("t").unwrap();
        for i in 0..6 {
            tour_avec_outils(&h, &s, &format!("Demande {i}"), &format!("Réponse {i}"), 4);
        }
        // L'ancienne fenêtre brute de 20 messages, pour prouver que le test
        // couvre bien le défaut : elle débute par un résultat d'outil.
        // 6 tours × 7 messages = 42 ; les 19 derniers débutent à l'indice 23,
        // soit le 3e message d'un tour : un résultat d'outil.
        let brut = h.messages(&s, 19).unwrap();
        assert_eq!(brut[0].role, Role::Tool, "le scénario doit reproduire l'orphelin");

        for limite in [1, 2, 3, 5, 20] {
            let conv = h.conversation(&s, limite).unwrap();
            assert!(conv.first().map_or(true, |m| m.role == Role::User), "limite {limite} : {conv:?}");
            assert!(conv.len() <= limite);
        }
    }

    /// Une demande restée sans réponse (limite atteinte, avant la correction)
    /// ne casse rien et la dernière réponse est conservée.
    #[test]
    fn les_tours_sans_reponse_finale_sont_tolerés() {
        let h = history();
        let s = h.create_session("t").unwrap();
        h.append(&s, &Message::user("Première demande")).unwrap();
        h.append(&s, &Message::user("Deuxième demande")).unwrap();
        h.append(&s, &Message::assistant("Réponse")).unwrap();
        let conv = h.conversation(&s, 20).unwrap();
        assert_eq!(conv.len(), 3);
        assert_eq!(conv[0].role, Role::User);
    }

    /// L'interface continue d'afficher l'historique complet.
    #[test]
    fn l_historique_complet_reste_disponible_pour_l_interface() {
        let h = history();
        let s = h.create_session("t").unwrap();
        tour_avec_outils(&h, &s, "Analyse", "Fini", 2);
        assert_eq!(h.messages(&s, 500).unwrap().len(), 1 + 1 + 2 + 1);
    }
}
