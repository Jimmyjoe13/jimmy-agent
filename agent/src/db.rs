//! Base SQLite locale : historique, mémoire, skills, serveurs MCP.
//!
//! Aucune donnée ne quitte la machine. Le schéma est créé au premier lancement
//! et evolve par `PRAGMA user_version`, sans dépendance à un outil de
//! migration : la V1 n'a pas besoin de plus.

use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

/// Version 2 : table `memory_semantic` (embeddings LM Studio). Toute table
/// ajoutée au script doit faire monter ce numéro, sinon les bases existantes
/// ne la reçoivent jamais (piège payé : « no such table: memory_semantic »).
pub const SCHEMA_VERSION: i64 = 2;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Ouvre (ou crée) la base et applique le schéma.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Base en mémoire, pour les tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;

        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version >= SCHEMA_VERSION {
            return Ok(());
        }

        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL DEFAULT 'Nouvelle session',
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role        TEXT NOT NULL,
                content     TEXT NOT NULL DEFAULT '',
                tool_name   TEXT,
                tool_calls  TEXT,
                tokens      INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

            CREATE TABLE IF NOT EXISTS memories (
                id           TEXT PRIMARY KEY,
                kind         TEXT NOT NULL DEFAULT 'episodic',
                content      TEXT NOT NULL,
                source       TEXT NOT NULL DEFAULT 'conversation',
                importance   REAL NOT NULL DEFAULT 0.5,
                created_at   TEXT NOT NULL,
                last_used_at TEXT,
                use_count    INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);

            CREATE TABLE IF NOT EXISTS memory_vectors (
                memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
                dim       INTEGER NOT NULL,
                vec       BLOB NOT NULL
            );

            -- Vecteurs sémantiques (LM Studio), à côté des vecteurs de
            -- hachage : ceux-ci restent le repli quand LM Studio est éteint.
            -- `model` évite de comparer deux espaces vectoriels différents.
            CREATE TABLE IF NOT EXISTS memory_semantic (
                memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                model     TEXT NOT NULL,
                vec       BLOB NOT NULL,
                PRIMARY KEY (memory_id, model)
            );

            CREATE TABLE IF NOT EXISTS skills (
                name         TEXT PRIMARY KEY,
                description  TEXT NOT NULL DEFAULT '',
                path         TEXT NOT NULL,
                author       TEXT NOT NULL DEFAULT 'jimmy',
                usage_count  INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mcp_servers (
                name        TEXT PRIMARY KEY,
                transport   TEXT NOT NULL,
                command     TEXT NOT NULL DEFAULT '',
                url         TEXT NOT NULL DEFAULT '',
                env         TEXT NOT NULL DEFAULT '{}',
                enabled     INTEGER NOT NULL DEFAULT 1,
                updated_at  TEXT NOT NULL
            );
            "#,
        )?;

        // Index plein texte : sert de complément lexical à la recherche
        // vectorielle. Si SQLite a été compilé sans FTS5, on continue sans :
        // la mémoire vectorielle suffit et la recherche devient un LIKE.
        let _ = self.conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content, memory_id UNINDEXED, tokenize = "unicode61 remove_diacritics 2"
            );
            "#,
        );
        let _ = self.conn.execute_batch(
            r#"
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(content, memory_id) VALUES (new.content, new.id);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                DELETE FROM memories_fts WHERE memory_id = old.id;
            END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                DELETE FROM memories_fts WHERE memory_id = old.id;
                INSERT INTO memories_fts(content, memory_id) VALUES (new.content, new.id);
            END;
            "#,
        );

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// `true` si la recherche plein texte est disponible.
    pub fn has_fts(&self) -> bool {
        self.conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memories_fts'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une base créée en version 1 (sans `memory_semantic`) doit recevoir la
    /// table à l'ouverture.
    #[test]
    fn migration_v1_vers_v2_cree_memory_semantic() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v1.db");
        // Base réelle, ramenée à l'état « version 1 » : sans la table, et
        // marquée v1 — exactement le cas d'une installation antérieure.
        drop(Db::open(&path).expect("création"));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("DROP TABLE memory_semantic; PRAGMA user_version = 1;").unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'memory_semantic'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
