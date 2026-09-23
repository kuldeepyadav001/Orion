//! SQLite persistence.
//!
//! One file holds everything: chats, messages, and later documents, vectors
//! (via sqlite-vec), the audit log and settings. Backup is copying one file.
//!
//! Two lessons carried over from Serina and fixed here:
//!
//! * **Recent messages, not the first N.** Serina's history loader used
//!   `ORDER BY created_at LIMIT 20`, which silently returned the *oldest*
//!   twenty messages, so long conversations lost all recent context. We order
//!   descending then reverse.
//! * **Persist the user turn before generating.** Serina wrote both messages
//!   only after the stream finished, so a closed tab lost the user's question.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{OrionError, Result};

/// Bumped whenever the schema changes; `migrate` applies the gap.
/// This is a real migration ladder, not `CREATE TABLE IF NOT EXISTS` — that
/// approach silently ignores every change to an existing table.
const SCHEMA_VERSION: i32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path` and bring the schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| OrionError::Db(format!("cannot create data dir: {e}")))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| OrionError::Db(format!("cannot open database: {e}")))?;

        // WAL keeps reads from blocking writes — matters once ingestion runs
        // in the background while the user is chatting.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| OrionError::Db(format!("cannot set WAL: {e}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| OrionError::Db(format!("cannot enable foreign keys: {e}")))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| OrionError::Db(format!("cannot set synchronous: {e}")))?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn user_version(&self) -> Result<i32> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| OrionError::Db(format!("cannot read user_version: {e}")))
    }

    fn migrate(&self) -> Result<()> {
        let current = self.user_version()?;
        if current >= SCHEMA_VERSION {
            return Ok(());
        }
        tracing::info!(from = current, to = SCHEMA_VERSION, "migrating schema");

        if current < 1 {
            self.conn
                .execute_batch(
                    r#"
                    CREATE TABLE sessions (
                        id          TEXT PRIMARY KEY,
                        title       TEXT NOT NULL DEFAULT 'New chat',
                        created_at  TEXT NOT NULL,
                        updated_at  TEXT NOT NULL
                    );

                    CREATE TABLE messages (
                        id          TEXT PRIMARY KEY,
                        session_id  TEXT NOT NULL
                                    REFERENCES sessions(id) ON DELETE CASCADE,
                        role        TEXT NOT NULL CHECK (role IN ('system','user','assistant')),
                        content     TEXT NOT NULL,
                        created_at  TEXT NOT NULL
                    );

                    CREATE INDEX idx_messages_session
                        ON messages(session_id, created_at DESC);

                    CREATE TABLE settings (
                        key   TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                    );
                    "#,
                )
                .map_err(|e| OrionError::Db(format!("migration v1 failed: {e}")))?;
        }

        if current < 2 {
            self.conn
                .execute_batch(
                    r#"
                    CREATE TABLE broker_audit_log (
                        id          TEXT PRIMARY KEY,
                        timestamp   TEXT NOT NULL,
                        domain      TEXT NOT NULL CHECK (domain IN ('trusted','untrusted')),
                        action      TEXT NOT NULL,
                        target      TEXT NOT NULL,
                        tier        TEXT NOT NULL CHECK (tier IN ('T0','T1','T2','T3','BLOCKED')),
                        decision    TEXT NOT NULL CHECK (decision IN ('ALLOWED','REJECTED','CONFIRMED','DENIED')),
                        reason      TEXT NOT NULL
                    );

                    CREATE INDEX idx_broker_audit_time
                        ON broker_audit_log(timestamp DESC);

                    CREATE TABLE broker_undo_journal (
                        id          TEXT PRIMARY KEY,
                        timestamp   TEXT NOT NULL,
                        action      TEXT NOT NULL,
                        target_path TEXT NOT NULL,
                        backup_data BLOB,
                        is_undone   INTEGER NOT NULL DEFAULT 0 CHECK (is_undone IN (0, 1))
                    );

                    CREATE INDEX idx_broker_undo_path
                        ON broker_undo_journal(target_path, timestamp DESC);
                    "#,
                )
                .map_err(|e| OrionError::Db(format!("migration v2 failed: {e}")))?;
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| OrionError::Db(format!("cannot bump user_version: {e}")))?;
        Ok(())
    }

    /// Borrow the underlying connection.
    ///
    /// `RagStore` layers the document tables onto this same database file
    /// rather than opening a second one: one file means one WAL and no
    /// chance of the two halves disagreeing about whether a write committed.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn create_session(&self, title: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO sessions (id, title, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![id, title, now],
            )
            .map_err(|e| OrionError::Db(format!("cannot create session: {e}")))?;
        Ok(id)
    }

    /// Append a message. Call this for the *user* turn before generation
    /// starts, so an interrupted stream still leaves the question on disk.
    pub fn add_message(&self, session_id: &str, role: &str, content: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO messages (id, session_id, role, content, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, session_id, role, content, now],
            )
            .map_err(|e| OrionError::Db(format!("cannot add message: {e}")))?;

        self.conn
            .execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, session_id],
            )
            .map_err(|e| OrionError::Db(format!("cannot touch session: {e}")))?;
        Ok(id)
    }

    /// The most recent `limit` messages, returned oldest-first for prompting.
    pub fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<StoredMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, role, content, created_at
                 FROM messages
                 WHERE session_id = ?1
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT ?2",
            )
            .map_err(|e| OrionError::Db(format!("cannot prepare query: {e}")))?;

        let rows = stmt
            .query_map(params![session_id, limit as i64], |r| {
                let ts: String = r.get(4)?;
                Ok(StoredMessage {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    created_at: DateTime::parse_from_rfc3339(&ts)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })
            .map_err(|e| OrionError::Db(format!("cannot query messages: {e}")))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| OrionError::Db(format!("bad row: {e}")))?);
        }
        // Descending for the LIMIT, reversed for chronological prompting.
        out.reverse();
        Ok(out)
    }
}

/// Platform-appropriate data directory, e.g.
/// `~/.local/share/orion` or `%APPDATA%\orion`.
pub fn data_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|d| d.join("orion"))
        .ok_or_else(|| OrionError::Db("cannot resolve a data directory".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Db, tempdir::TempDir) {
        // Using a plain temp path keeps the test dependency-light.
        let dir = std::env::temp_dir().join(format!("orion-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        (db, tempdir::TempDir(dir))
    }

    mod tempdir {
        pub struct TempDir(pub std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn migrates_to_current_version() {
        let (db, _g) = temp_db();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn migration_is_idempotent() {
        let (db, _g) = temp_db();
        db.migrate().unwrap();
        db.migrate().unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn roundtrips_messages_in_order() {
        let (db, _g) = temp_db();
        let s = db.create_session("t").unwrap();
        db.add_message(&s, "user", "first").unwrap();
        db.add_message(&s, "assistant", "second").unwrap();
        db.add_message(&s, "user", "third").unwrap();

        let msgs = db.recent_messages(&s, 10).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content, "first");
        assert_eq!(msgs[2].content, "third");
    }

    /// Regression test for the Serina B-1 defect: the loader must return the
    /// most recent messages, not the oldest ones.
    #[test]
    fn recent_messages_returns_newest_not_oldest() {
        let (db, _g) = temp_db();
        let s = db.create_session("t").unwrap();
        for i in 0..10 {
            db.add_message(&s, "user", &format!("msg{i}")).unwrap();
        }

        let msgs = db.recent_messages(&s, 3).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content, "msg7");
        assert_eq!(msgs[2].content, "msg9", "must return the newest messages");
    }

    #[test]
    fn cascade_delete_removes_messages() {
        let (db, _g) = temp_db();
        let s = db.create_session("t").unwrap();
        db.add_message(&s, "user", "x").unwrap();
        db.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![s])
            .unwrap();

        let n: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "messages must cascade with their session");
    }
}
