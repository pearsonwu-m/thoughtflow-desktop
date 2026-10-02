//! Local SQLite storage for thoughts, conversations, plans, tasks, and prompts.
//!
//! Everything stays on this machine. Full-text search (FTS5) powers both the
//! history search and the related-memory lookup that decides which past notes
//! are offered to Claude as context.

pub mod export;
pub mod models;
pub mod plans;
pub mod search;
pub mod thoughts;

use rusqlite::Connection;
use std::path::Path;

const SCHEMA_V1: &str = r#"
CREATE TABLE thoughts (
    id                TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    raw_input         TEXT NOT NULL,
    summary           TEXT NOT NULL DEFAULT '',
    tags              TEXT NOT NULL DEFAULT '[]',
    mode              TEXT NOT NULL DEFAULT 'think',
    include_in_memory INTEGER NOT NULL DEFAULT 1,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);
CREATE INDEX thoughts_updated_at ON thoughts(updated_at DESC);

CREATE TABLE messages (
    id           TEXT PRIMARY KEY,
    thought_id   TEXT NOT NULL REFERENCES thoughts(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    kind         TEXT NOT NULL DEFAULT 'message',
    mode         TEXT NOT NULL DEFAULT 'think',
    display_text TEXT NOT NULL DEFAULT '',
    provider     TEXT NOT NULL DEFAULT 'anthropic',
    payload      TEXT NOT NULL,
    model        TEXT,
    context_ids  TEXT NOT NULL DEFAULT '[]',
    truncated    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    UNIQUE (thought_id, seq)
);

CREATE TABLE plans (
    id          TEXT PRIMARY KEY,
    thought_id  TEXT REFERENCES thoughts(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    objective   TEXT NOT NULL DEFAULT '',
    why         TEXT NOT NULL DEFAULT '',
    steps       TEXT NOT NULL DEFAULT '[]',
    next_action TEXT NOT NULL DEFAULT '',
    obstacles   TEXT NOT NULL DEFAULT '[]',
    deadline    TEXT,
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'done', 'archived')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    thought_id   TEXT REFERENCES thoughts(id) ON DELETE CASCADE,
    plan_id      TEXT REFERENCES plans(id) ON DELETE SET NULL,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    priority     TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('high', 'medium', 'low')),
    effort       TEXT NOT NULL DEFAULT 'short' CHECK (effort IN ('quick', 'short', 'medium', 'long')),
    due_date     TEXT,
    completed    INTEGER NOT NULL DEFAULT 0,
    completed_at INTEGER,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE prompts (
    id         TEXT PRIMARY KEY,
    thought_id TEXT NOT NULL REFERENCES thoughts(id) ON DELETE CASCADE,
    message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
    content    TEXT NOT NULL,
    edited     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE thoughts_fts USING fts5(
    title, raw_input, summary, tags,
    content = 'thoughts', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER thoughts_ai AFTER INSERT ON thoughts BEGIN
    INSERT INTO thoughts_fts(rowid, title, raw_input, summary, tags)
    VALUES (new.rowid, new.title, new.raw_input, new.summary, new.tags);
END;
CREATE TRIGGER thoughts_ad AFTER DELETE ON thoughts BEGIN
    INSERT INTO thoughts_fts(thoughts_fts, rowid, title, raw_input, summary, tags)
    VALUES ('delete', old.rowid, old.title, old.raw_input, old.summary, old.tags);
END;
CREATE TRIGGER thoughts_au AFTER UPDATE ON thoughts BEGIN
    INSERT INTO thoughts_fts(thoughts_fts, rowid, title, raw_input, summary, tags)
    VALUES ('delete', old.rowid, old.title, old.raw_input, old.summary, old.tags);
    INSERT INTO thoughts_fts(rowid, title, raw_input, summary, tags)
    VALUES (new.rowid, new.title, new.raw_input, new.summary, new.tags);
END;

CREATE VIRTUAL TABLE messages_fts USING fts5(
    display_text,
    content = 'messages', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, display_text) VALUES (new.rowid, new.display_text);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, display_text) VALUES ('delete', old.rowid, old.display_text);
END;
CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, display_text) VALUES ('delete', old.rowid, old.display_text);
    INSERT INTO messages_fts(rowid, display_text) VALUES (new.rowid, new.display_text);
END;
"#;

/// Ordered migrations; `PRAGMA user_version` records how many have run.
const MIGRATIONS: &[&str] = &[SCHEMA_V1];

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    configure(&conn).unwrap();
    migrate(&conn).unwrap();
    conn
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // Overwrite deleted content so "delete" means gone from the file too.
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;",
            i + 1
        ))?;
    }
    Ok(())
}

/// Flushes the write-ahead log into the main file and truncates it, so
/// deleted rows don't linger in the WAL.
pub fn checkpoint(conn: &Connection) -> rusqlite::Result<()> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .or_else(|e| match e {
            // In-memory databases have no WAL.
            rusqlite::Error::QueryReturnedNoRows => Ok(()),
            other => Err(other),
        })
}

pub fn json_vec(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let conn = open_in_memory();
        migrate(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn opens_a_file_database_with_wal() {
        let dir = std::env::temp_dir().join(format!("tf-db-{}", crate::util::new_id()));
        let path = dir.join("thoughtflow.db");
        let conn = open(&path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        checkpoint(&conn).unwrap();
        drop(conn);
        std::fs::remove_dir_all(dir).ok();
    }
}
