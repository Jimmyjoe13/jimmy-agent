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