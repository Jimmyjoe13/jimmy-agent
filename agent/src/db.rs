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
/// Version 6 : sessions épinglées, archivées, titre verrouillé.
/// Version 7 : date de dernière lecture (`read_at`, messages non lus).
pub const SCHEMA_VERSION: i64 = 7;

/// Titres génériques donnés à la création d'une session, remplacés dès
/// qu'une vraie demande arrive (`History::auto_title`).
pub const GENERIC_TITLES: [&str; 3] = ["Nouvelle session", "Session vocale", ""];

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
                updated_at  TEXT NOT NULL,
                project     TEXT,
                pinned       INTEGER NOT NULL DEFAULT 0,
                archived     INTEGER NOT NULL DEFAULT 0,
                title_locked INTEGER NOT NULL DEFAULT 0,
                read_at      TEXT
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

            -- v4 : carnet de bord du contexte (`core::recap`). Résumé glissant
            -- d'une conversation : `folded` = messages déjà fondus dans le
            -- résumé, `seen` = messages déjà relus pour les chantiers.
            CREATE TABLE IF NOT EXISTS session_recaps (
                session_id  TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                summary     TEXT NOT NULL DEFAULT '',
                folded      INTEGER NOT NULL DEFAULT 0,
                seen        INTEGER NOT NULL DEFAULT 0,
                updated_at  TEXT NOT NULL
            );

            -- Chantiers suivis d'une conversation à l'autre : statut
            -- `ouvert`, `attente` (de l'utilisateur) ou `fini`.
            CREATE TABLE IF NOT EXISTS chantiers (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'ouvert',
                next_step   TEXT NOT NULL DEFAULT '',
                project     TEXT,
                session_id  TEXT,
                updated_at  TEXT NOT NULL
            );

            -- v5 : carnet de travail d'une conversation (`core::notes`) : les
            -- acquis vérifiés des tours outillés, relus au tour suivant.
            CREATE TABLE IF NOT EXISTS work_notes (
                session_id  TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                notes       TEXT NOT NULL DEFAULT '',
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

        // v3 : projet (dossier) rattaché à une conversation, onglet Chat. Une
        // base v2 a déjà la table : `CREATE TABLE IF NOT EXISTS` n'y ajoute
        // pas la colonne (piège 30), d'où l'ALTER explicite.
        let has_project = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'project'")?
            .exists([])?;
        if !has_project {
            self.conn.execute_batch("ALTER TABLE sessions ADD COLUMN project TEXT;")?;
        }

        // v6 : sessions épinglées, archivées, titre verrouillé (renommé par
        // l'utilisateur). Même règle : ALTER explicite pour les bases existantes.
        for column in ["pinned", "archived", "title_locked"] {
            let exists = self
                .conn
                .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = ?1")?
                .exists([column])?;
            if !exists {
                self.conn
                    .execute_batch(&format!("ALTER TABLE sessions ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0;"))?;
            }
        }
        if version < 6 {
            self.retitle_generic_sessions()?;
        }
        // v7 : date de dernière lecture. Les conversations d'avant sont
        // considérées lues (sinon 195 pastilles « non lu » d'un coup).
        let has_read_at = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'read_at'")?
            .exists([])?;
        if !has_read_at {
            self.conn.execute_batch("ALTER TABLE sessions ADD COLUMN read_at TEXT;")?;
        }
        if version < 7 {
            self.conn
                .execute_batch("UPDATE sessions SET read_at = updated_at WHERE read_at IS NULL;")?;
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Rattrapage unique (passage en v6) : une session restée « Session
    /// vocale » / « Nouvelle session » prend sa première demande d'au moins
    /// trois mots (« Jimy. » ne fait pas un titre), sinon sa première demande.
    /// Audit du 21 octobre : 89 sessions sur 195 sans vrai titre.
    fn retitle_generic_sessions(&self) -> Result<()> {
        // Contenu sur une ligne : retours à la ligne remplacés par des espaces.
        const FLAT: &str = "trim(replace(replace(m.content, char(13), ''), char(10), ' '))";
        let first = |filter: &str| {
            format!(
                "(SELECT substr({FLAT}, 1, 60) FROM messages m
                  WHERE m.session_id = sessions.id AND m.role = 'user' AND {FLAT} != '' {filter}
                  ORDER BY m.created_at LIMIT 1)"
            )
        };
        let three_words = format!("AND length({FLAT}) - length(replace({FLAT}, ' ', '')) >= 2");
        let generic = GENERIC_TITLES.iter().map(|t| format!("'{t}'")).collect::<Vec<_>>().join(", ");
        self.conn.execute(
            &format!(
                "UPDATE sessions SET title = coalesce({}, {})
                 WHERE title IN ({generic}) AND {} IS NOT NULL",
                first(&three_words),
                first(""),
                first("")
            ),
            [],
        )?;
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

    /// Une base v2 (sessions sans colonne `project`) doit la recevoir.
    #[test]
    fn migration_v2_vers_v3_ajoute_le_projet_des_sessions() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-v3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v2.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'x', \
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL); \
                 INSERT INTO sessions VALUES ('s1', 't', 'a', 'a'); PRAGMA user_version = 2;",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        let project: Option<String> = db
            .conn()
            .query_row("SELECT project FROM sessions WHERE id = 's1'", [], |r| r.get(0))
            .expect("la colonne project doit exister");
        assert!(project.is_none(), "les sessions existantes n'ont pas de projet");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Une base v3 (la vraie base au 10 octobre) reçoit le carnet de bord du
    /// contexte : résumés glissants et chantiers.
    #[test]
    fn migration_v3_vers_v4_cree_le_carnet_de_bord() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-v4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v3.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'x', \
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL, project TEXT); PRAGMA user_version = 3;",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        for table in ["session_recaps", "chantiers"] {
            let n: i64 = db
                .conn()
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "table {table} absente");
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Une base v6 reçoit `read_at` ; ses conversations sont marquées lues.
    #[test]
    fn migration_v6_vers_v7_conversations_existantes_lues() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-v7-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v6.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'x',
                     created_at TEXT NOT NULL, updated_at TEXT NOT NULL, project TEXT,
                     pinned INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0,
                     title_locked INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s', 'Titre', 'a', '2026-10-20T10:00:00+00:00');
                 PRAGMA user_version = 6;",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        let read_at: Option<String> =
            db.conn().query_row("SELECT read_at FROM sessions WHERE id = 's'", [], |r| r.get(0)).unwrap();
        assert_eq!(read_at.as_deref(), Some("2026-10-20T10:00:00+00:00"));
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Une base v5 (la vraie base au 21 octobre) reçoit épinglage, archivage
    /// et verrou de titre ; les sessions au titre générique prennent leur
    /// première vraie demande (au moins trois mots) comme titre.
    #[test]
    fn migration_v5_vers_v6_sessions_titrees_et_classables() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-v6-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v5.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'x',
                     created_at TEXT NOT NULL, updated_at TEXT NOT NULL, project TEXT);
                 CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL,
                     content TEXT NOT NULL DEFAULT '', tool_name TEXT, tool_calls TEXT,
                     tokens INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
                 INSERT INTO sessions VALUES ('v', 'Session vocale', 'a', 'a', NULL);
                 INSERT INTO sessions VALUES ('n', 'Nouvelle session', 'a', 'a', NULL);
                 INSERT INTO sessions VALUES ('k', 'Mon titre', 'a', 'a', NULL);
                 INSERT INTO messages VALUES ('1', 'v', 'user', 'Jimy.', NULL, NULL, 0, '1');
                 INSERT INTO messages VALUES ('2', 'v', 'user', 'prépare la relance
de Caroline', NULL, NULL, 0, '2');
                 INSERT INTO messages VALUES ('3', 'k', 'user', 'une demande quelconque', NULL, NULL, 0, '1');
                 PRAGMA user_version = 5;",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        let title = |id: &str| -> String {
            db.conn().query_row("SELECT title FROM sessions WHERE id = ?1", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(title("v"), "prépare la relance de Caroline");
        assert_eq!(title("n"), "Nouvelle session", "aucune demande : titre inchangé");
        assert_eq!(title("k"), "Mon titre");
        let cols: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name IN ('pinned', 'archived', 'title_locked')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 3);
        let version: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Une base v4 (la vraie base au 14 octobre) reçoit le carnet de travail.
    #[test]
    fn migration_v4_vers_v5_cree_le_carnet_de_travail() {
        let dir = std::env::temp_dir().join(format!("jimmy-db-v5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v4.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'x',                  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, project TEXT); PRAGMA user_version = 4;",
            )
            .unwrap();
        }
        let db = Db::open(&path).expect("ouverture");
        let n: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='work_notes'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "table work_notes absente");
        let version: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
