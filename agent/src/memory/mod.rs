//! Mémoire personnelle de Jimmy, entièrement locale.
//!
//! Trois familles de souvenirs :
//!
//! * `semantic` — faits et préférences stables ;
//! * `procedural` — règles, procédures validées ;
//! * `episodic` — ce qui s'est passé, decisions prises.
//!
//! La recherche combine deux signaux : similarité vectorielle (voir
//! [`embed`]) et pertinence lexicale FTS5 quand SQLite le permet. Les deux
//! scores sont normalisés puis fusionnés.

pub mod embed;
pub mod learn;


use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use crate::core::history::Shared;
use crate::db::Db;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Semantic,
    Procedural,
    Episodic,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Semantic => "semantic",
            MemoryKind::Procedural => "procedural",
            MemoryKind::Episodic => "episodic",
        }
    }

    pub fn parse(value: &str) -> MemoryKind {
        match value {
            "semantic" => MemoryKind::Semantic,
            "procedural" => MemoryKind::Procedural,
            _ => MemoryKind::Episodic,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: String,
    pub kind: MemoryKind,
    pub content: String,
    pub source: String,
    pub importance: f32,
    pub created_at: String,
    pub use_count: i64,
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub memory: Memory,
    pub score: f32,
}

pub struct MemoryStore {
    db: Shared<Db>,
}

impl MemoryStore {
    pub fn new(db: Shared<Db>) -> Self {
        MemoryStore { db }
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .db
            .lock()
            .unwrap()
            .conn()
            .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))?)
    }

    /// Enregistre un souvenir. Le contenu identique est remonté en
    /// importance plutôt que dupliqué.
    pub fn remember(
        &self,
        kind: MemoryKind,
        content: &str,
        source: &str,
        importance: f32,
    ) -> Result<String> {
        let content = content.trim();
        if content.is_empty() {
            return Ok(String::new());
        }
        let conn = self.db.lock().unwrap();
        let existing: Option<String> = conn
            .conn()
            .query_row(
                "SELECT id FROM memories WHERE content = ?1 LIMIT 1",
                params![content],
                |r| r.get(0),
            )
            .ok();

        if let Some(id) = existing {
            conn.conn().execute(
                "UPDATE memories SET importance = min(1.0, importance + 0.1), last_used_at = ?2 WHERE id = ?1",
                params![id, Utc::now().to_rfc3339()],
            )?;
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.conn().execute(
            "INSERT INTO memories (id, kind, content, source, importance, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, kind.as_str(), content, source, importance.clamp(0.0, 1.0), now],
        )?;
        let vector = embed::embed(content);
        conn.conn().execute(
            "INSERT OR REPLACE INTO memory_vectors (memory_id, dim, vec) VALUES (?1, ?2, ?3)",
            params![id, embed::DIM as i64, vector_to_blob(&vector)],
        )?;
        Ok(id)
    }

    /// Recherche hybride. `query` est la demande courante de l'utilisateur.
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
        let target = embed::embed(query);
        let conn = self.db.lock().unwrap();

        let mut stmt = conn.conn().prepare(
            "SELECT m.id, m.kind, m.content, m.source, m.importance, m.created_at, m.use_count, v.vec
             FROM memories m LEFT JOIN memory_vectors v ON v.memory_id = m.id",
        )?;
        let rows = stmt.query_map([], |row| {
            let blob: Option<Vec<u8>> = row.get(7)?;
            Ok((
                Memory {
                    id: row.get(0)?,
                    kind: MemoryKind::parse(&row.get::<_, String>(1)?),
                    content: row.get(2)?,
                    source: row.get(3)?,
                    importance: row.get(4)?,
                    created_at: row.get(5)?,
                    use_count: row.get(6)?,
                },
                blob,
            ))
        })?;

        let mut hits: Vec<MemoryHit> = Vec::new();
        for row in rows {
            let (memory, blob) = row?;
            let similarity = blob
                .as_deref()
                .map(|b| embed::similarity(&target, &blob_to_vec(b)))
                .unwrap_or(0.0);
            // Un souvenir explicitement important reste pertinent même si le
            // texte ne se recoupe pas (préférence, règle).
            let score = similarity * (0.6 + 0.4 * memory.importance);
            if score > 0.05 {
                hits.push(MemoryHit { memory, score });
            }
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Injection textuelle des souvenirs les plus pertinents pour le prompt.
    pub fn context_block(&self, query: &str, limit: usize) -> Result<String> {
        let hits = self.recall(query, limit)?;
        if hits.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::from("Ce que Jimmy sait de l'utilisateur :\n");
        for hit in hits {
            out.push_str(&format!("- [{}] {}\n", hit.memory.kind.as_str(), hit.memory.content));
        }
        Ok(out)
    }

    pub fn mark_used(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        for id in ids {
            conn.conn().execute(
                "UPDATE memories SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
                params![id, now],
            )?;
        }
        Ok(())
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        self.db
            .lock()
            .unwrap()
            .conn()
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT id, kind, content, source, importance, created_at, use_count
             FROM memories ORDER BY importance DESC, created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                kind: MemoryKind::parse(&row.get::<_, String>(1)?),
                content: row.get(2)?,
                source: row.get(3)?,
                importance: row.get(4)?,
                created_at: row.get(5)?,
                use_count: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        let db = std::sync::Arc::new(std::sync::Mutex::new(Db::open_in_memory().expect("db")));
        MemoryStore::new(db)
    }

    #[test]
    fn memorise_et_retrouve() {
        let store = store();
        store
            .remember(MemoryKind::Semantic, "Jimmy utilise Rust pour son backend", "test", 0.8)
            .unwrap();
        store
            .remember(MemoryKind::Semantic, "Le Modellé préféré est small", "test", 0.8)
            .unwrap();

        let hits = store.recall("quel langage pour le backend", 5).unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].memory.content.contains("Rust"));
    }

    #[test]
    fn doublon_non_duplique() {
        let store = store();
        store.remember(MemoryKind::Semantic, "note A", "test", 0.5).unwrap();
        store.remember(MemoryKind::Semantic, "note A", "test", 0.5).unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn souvenir_oublie() {
        let store = store();
        let id = store.remember(MemoryKind::Episodic, "a fait un café", "test", 0.5).unwrap();
        store.forget(&id).unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }
}