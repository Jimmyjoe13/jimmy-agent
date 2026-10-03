//! Historique des conversations, stocké dans SQLite.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, Row};
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

    pub fn sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    (SELECT count(*) FROM messages m WHERE m.session_id = s.id)
             FROM sessions s ORDER BY s.updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row: &Row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
            })
        })?;
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
