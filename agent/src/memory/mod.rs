//! Mémoire personnelle de Jimmy, entièrement locale.
//!
//! Trois familles de souvenirs :
//!
//! * `semantic` — faits et préférences stables ;
//! * `procedural` — règles, procédures validées ;
//! * `episodic` — ce qui s'est passé, decisions prises.
//!
//! Recherche vectorielle à deux niveaux :
//!
//! * **sémantique** ([`semantic`]) — embeddings LM Studio, rapprochent le
//!   sens (« voiture » ≈ « véhicule ») ;
//! * **hachage** ([`embed`]) — toujours calculé, sans dépendance : c'est le
//!   repli quand LM Studio ne répond pas.
//!
//! Chaque souvenir garde son vecteur de hachage ; le vecteur sémantique est
//! ajouté quand LM Studio est disponible (à l'écriture, ou par
//! [`MemoryStore::reindex_semantic`] au démarrage).

pub mod embed;
pub mod learn;
pub mod semantic;
pub mod sort;
pub mod vault;

use std::sync::Arc;

use semantic::SemanticEmbedder;


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
    semantic: Option<Arc<SemanticEmbedder>>,
}

/// Similarité sémantique minimale. Les modèles MiniLM donnent ~0,1 à 0,3
/// entre phrases sans rapport : en dessous de 0,35, c'est du bruit.
const SEMANTIC_MIN: f32 = 0.35;

/// Souvenirs du socle, chargés à chaque demande (les plus importants).
pub const BASE_MEMORY_COUNT: usize = 8;

/// Taille maximale du bloc mémoire injecté dans le prompt.
pub const MEMORY_BLOCK_MAX_CHARS: usize = 1200;

impl MemoryStore {
    /// Mémoire sans embeddings sémantiques (hachage seul).
    pub fn new(db: Shared<Db>) -> Self {
        MemoryStore { db, semantic: None }
    }

    /// Mémoire avec embeddings sémantiques (LM Studio), repli sur le hachage.
    pub fn with_semantic(db: Shared<Db>, semantic: Option<SemanticEmbedder>) -> Self {
        MemoryStore {
            db,
            semantic: semantic.map(Arc::new),
        }
    }

    /// Modèle sémantique configuré, s'il y en a un.
    pub fn semantic_model(&self) -> Option<String> {
        self.semantic.as_ref().map(|s| s.model().to_string())
    }

    /// Enregistre un souvenir, puis son vecteur sémantique si LM Studio
    /// répond. À préférer à [`MemoryStore::remember`] dans le code async.
    pub async fn remember_indexed(
        &self,
        kind: MemoryKind,
        content: &str,
        source: &str,
        importance: f32,
    ) -> Result<String> {
        let id = self.remember(kind, content, source, importance)?;
        if id.is_empty() {
            return Ok(id);
        }
        if let Some(semantic) = &self.semantic {
            if let Some(vector) = semantic.embed(content.trim()).await {
                self.store_semantic(&id, semantic.model(), &vector)?;
            }
        }
        Ok(id)
    }

    fn store_semantic(&self, id: &str, model: &str, vector: &[f32]) -> Result<()> {
        self.db.lock().unwrap().conn().execute(
            "INSERT OR REPLACE INTO memory_semantic (memory_id, model, vec) VALUES (?1, ?2, ?3)",
            params![id, model, vector_to_blob(vector)],
        )?;
        Ok(())
    }

    /// Calcule les vecteurs sémantiques manquants (souvenirs écrits avant
    /// l'activation, ou pendant une absence de LM Studio). S'arrête au premier
    /// échec : LM Studio est alors indisponible. Renvoie le nombre indexé.
    pub async fn reindex_semantic(&self) -> usize {
        let Some(semantic) = &self.semantic else {
            return 0;
        };
        let pending: Vec<(String, String)> = {
            let conn = self.db.lock().unwrap();
            let Ok(mut stmt) = conn.conn().prepare(
                "SELECT m.id, m.content FROM memories m
                 WHERE NOT EXISTS (SELECT 1 FROM memory_semantic s
                                   WHERE s.memory_id = m.id AND s.model = ?1)",
            ) else {
                return 0;
            };
            stmt.query_map(params![semantic.model()], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        };
        let mut done = 0;
        for (id, content) in pending {
            let Some(vector) = semantic.embed(&content).await else {
                break;
            };
            if self.store_semantic(&id, semantic.model(), &vector).is_ok() {
                done += 1;
            }
        }
        done
    }

    /// Rappel sémantique si LM Studio répond, par hachage sinon.
    pub async fn recall_semantic(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
        let target = match &self.semantic {
            Some(semantic) => semantic
                .embed(query)
                .await
                .map(|v| (semantic.model().to_string(), v)),
            None => None,
        };
        self.recall_with(query, target, limit)
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

    /// Recherche par hachage seul (synchrone). `query` est la demande
    /// courante de l'utilisateur.
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
        self.recall_with(query, None, limit)
    }

    /// Recherche. `semantic` = (modèle, vecteur de la requête) : utilisé pour
    /// les souvenirs qui ont un vecteur de ce modèle ; les autres sont
    /// comparés par hachage.
    fn recall_with(
        &self,
        query: &str,
        semantic: Option<(String, Vec<f32>)>,
        limit: usize,
    ) -> Result<Vec<MemoryHit>> {
        let target = embed::embed(query);
        let model = semantic.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
        let conn = self.db.lock().unwrap();

        let mut stmt = conn.conn().prepare(
            "SELECT m.id, m.kind, m.content, m.source, m.importance, m.created_at, m.use_count, v.vec, s.vec
             FROM memories m
             LEFT JOIN memory_vectors v ON v.memory_id = m.id
             LEFT JOIN memory_semantic s ON s.memory_id = m.id AND s.model = ?1",
        )?;
        let rows = stmt.query_map(params![model], |row| {
            let blob: Option<Vec<u8>> = row.get(7)?;
            let semantic_blob: Option<Vec<u8>> = row.get(8)?;
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
                semantic_blob,
            ))
        })?;

        let mut hits: Vec<MemoryHit> = Vec::new();
        for row in rows {
            let (memory, blob, semantic_blob) = row?;
            // Sémantique si possible (même modèle, même dimension), hachage sinon.
            let semantic_similarity = match (&semantic, semantic_blob.as_deref()) {
                (Some((_, query_vec)), Some(b)) => {
                    let stored = blob_to_vec(b);
                    (stored.len() == query_vec.len()).then(|| embed::similarity(query_vec, &stored))
                }
                _ => None,
            };
            let similarity = match semantic_similarity {
                Some(sim) if sim < SEMANTIC_MIN => continue,
                Some(sim) => sim,
                None => blob
                    .as_deref()
                    .map(|b| embed::similarity(&target, &blob_to_vec(b)))
                    .unwrap_or(0.0),
            };
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
    /// Bloc mémoire du prompt : un **socle** (les souvenirs les plus
    /// importants, toujours chargés) puis le rappel lié à la demande.
    ///
    /// Le rappel seul ne suffisait pas : sur « installe-le », il ne ramène
    /// rien d'utile, et Jimmy repartait sans rien savoir de l'utilisateur. Le
    /// tout reste borné par `MEMORY_BLOCK_MAX_CHARS` (≈ 300 jetons).
    pub async fn context_block(&self, query: &str, limit: usize) -> Result<String> {
        let base = self.list(BASE_MEMORY_COUNT)?;
        let hits = self.recall_semantic(query, limit).await?;
        let mut seen = std::collections::HashSet::new();
        let mut lines = Vec::new();
        let mut used = 0usize;
        // Le rappel passe d'abord : il est propre à la demande ; le socle
        // complète dans le budget restant.
        for memory in hits.into_iter().map(|h| h.memory).chain(base) {
            if !seen.insert(memory.id.clone()) {
                continue;
            }
            let line = format!("- [{}] {}", memory.kind.as_str(), memory.content);
            let size = line.chars().count() + 1;
            if used + size > MEMORY_BLOCK_MAX_CHARS {
                continue;
            }
            used += size;
            lines.push(line);
        }
        if lines.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("Ce que Jimy sait de l'utilisateur :\n{}\n", lines.join("\n")))
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
        // Explicite : la cascade dépend de `PRAGMA foreign_keys`.
        self.db
            .lock()
            .unwrap()
            .conn()
            .execute("DELETE FROM memory_semantic WHERE memory_id = ?1", params![id])?;
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

    /// Leçons récentes (source « lesson »), de la plus récente : le journal
    /// d'expérience relu par la revue périodique (module `growth`).
    pub fn lessons(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.conn().prepare(
            "SELECT id, kind, content, source, importance, created_at, use_count
             FROM memories WHERE source = 'lesson'
             ORDER BY created_at DESC LIMIT ?1",
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

    /// La mémoire de base (souvenirs les plus importants) est chargée même
    /// quand la demande ne ressemble à rien (« installe-le »), sans doublon
    /// avec le rappel, et dans un budget de caractères.
    #[tokio::test]
    async fn le_socle_de_memoire_est_toujours_charge() {
        let store = store();
        store
            .remember(MemoryKind::Semantic, "L'utilisateur s'appelle Jimmy et parle français", "test", 0.9)
            .unwrap();
        store.remember(MemoryKind::Procedural, "Toujours répondre court", "test", 0.8).unwrap();
        for i in 0..30 {
            store
                .remember(MemoryKind::Episodic, &format!("souvenir mineur numéro {i} {}", "z".repeat(80)), "test", 0.1)
                .unwrap();
        }
        let block = store.context_block("installe-le", 6).await.unwrap();
        assert!(block.contains("s'appelle Jimmy"), "{block}");
        assert!(block.contains("répondre court"), "{block}");
        assert!(block.chars().count() <= MEMORY_BLOCK_MAX_CHARS + 80, "{} caractères", block.chars().count());
        // Pas de doublon si le rappel retrouve aussi un souvenir du socle.
        let block = store.context_block("Jimmy parle français", 6).await.unwrap();
        assert_eq!(block.matches("s'appelle Jimmy").count(), 1, "{block}");
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