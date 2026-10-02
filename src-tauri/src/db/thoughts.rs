//! Thoughts, their conversation turns, and generated prompts.

use super::models::{Message, Prompt, StoredMessage, Thought, ThoughtDetail};
use super::{json_vec, plans};
use crate::ai::Mode;
use rusqlite::{params, Connection, OptionalExtension, Row};

pub const THOUGHT_COLUMNS: &str =
    "id, title, raw_input, summary, tags, mode, include_in_memory, created_at, updated_at";

pub fn thought_from_row(row: &Row) -> rusqlite::Result<Thought> {
    Ok(Thought {
        id: row.get(0)?,
        title: row.get(1)?,
        raw_input: row.get(2)?,
        summary: row.get(3)?,
        tags: json_vec(&row.get::<_, String>(4)?),
        mode: Mode::parse(&row.get::<_, String>(5)?),
        include_in_memory: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn insert(conn: &Connection, t: &Thought) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO thoughts (id, title, raw_input, summary, tags, mode, include_in_memory, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            t.id,
            t.title,
            t.raw_input,
            t.summary,
            serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into()),
            t.mode.as_str(),
            t.include_in_memory,
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Thought>> {
    conn.query_row(
        &format!("SELECT {THOUGHT_COLUMNS} FROM thoughts WHERE id = ?1"),
        [id],
        thought_from_row,
    )
    .optional()
}

pub fn list_recent(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<Thought>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THOUGHT_COLUMNS} FROM thoughts ORDER BY updated_at DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map([limit], thought_from_row)?;
    rows.collect()
}

pub fn touch(
    conn: &Connection,
    id: &str,
    mode: Mode,
    summary: Option<&str>,
    now: i64,
) -> rusqlite::Result<()> {
    match summary {
        Some(s) => conn.execute(
            "UPDATE thoughts SET mode = ?2, summary = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, mode.as_str(), s, now],
        )?,
        None => conn.execute(
            "UPDATE thoughts SET mode = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, mode.as_str(), now],
        )?,
    };
    Ok(())
}

/// Turns "no row was updated" into `QueryReturnedNoRows` so callers can
/// report a missing record.
pub fn ensure_changed(n: usize) -> rusqlite::Result<()> {
    if n == 0 {
        Err(rusqlite::Error::QueryReturnedNoRows)
    } else {
        Ok(())
    }
}

pub fn rename(conn: &Connection, id: &str, title: &str, now: i64) -> rusqlite::Result<()> {
    let n = conn.execute(
        "UPDATE thoughts SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now],
    )?;
    ensure_changed(n)
}

pub fn set_tags(conn: &Connection, id: &str, tags: &[String], now: i64) -> rusqlite::Result<()> {
    let n = conn.execute(
        "UPDATE thoughts SET tags = ?2, updated_at = ?3 WHERE id = ?1",
        params![
            id,
            serde_json::to_string(tags).unwrap_or_else(|_| "[]".into()),
            now
        ],
    )?;
    ensure_changed(n)
}

pub fn set_include_in_memory(conn: &Connection, id: &str, include: bool) -> rusqlite::Result<()> {
    let n = conn.execute(
        "UPDATE thoughts SET include_in_memory = ?2 WHERE id = ?1",
        params![id, include],
    )?;
    ensure_changed(n)
}

/// Deletes a thought and everything derived from it (conversation, prompts,
/// plans, tasks) via `ON DELETE CASCADE`.
pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM thoughts WHERE id = ?1", [id])? > 0)
}

// --- Messages ---------------------------------------------------------------

pub const KIND_MESSAGE: &str = "message";
pub const KIND_CLARIFY: &str = "clarify";
pub const KIND_MODE_REQUEST: &str = "modeRequest";
pub const KIND_MODE_SWITCH: &str = "modeSwitch";

const MESSAGE_COLUMNS: &str = "id, thought_id, seq, role, kind, mode, display_text, model, context_ids, truncated, created_at, provider, payload";

fn stored_from_row(row: &Row) -> rusqlite::Result<StoredMessage> {
    let payload_raw: String = row.get(12)?;
    Ok(StoredMessage {
        message: Message {
            id: row.get(0)?,
            thought_id: row.get(1)?,
            seq: row.get(2)?,
            role: row.get(3)?,
            kind: row.get(4)?,
            mode: Mode::parse(&row.get::<_, String>(5)?),
            text: row.get(6)?,
            model: row.get(7)?,
            context_ids: json_vec(&row.get::<_, String>(8)?),
            truncated: row.get(9)?,
            created_at: row.get(10)?,
        },
        provider: row.get(11)?,
        payload: serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null),
    })
}

pub fn load_messages(conn: &Connection, thought_id: &str) -> rusqlite::Result<Vec<StoredMessage>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages WHERE thought_id = ?1 ORDER BY seq ASC"
    ))?;
    let rows = stmt.query_map([thought_id], stored_from_row)?;
    rows.collect()
}

pub fn next_seq(conn: &Connection, thought_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), -1) + 1 FROM messages WHERE thought_id = ?1",
        [thought_id],
        |r| r.get(0),
    )
}

pub fn insert_message(conn: &Connection, m: &StoredMessage) -> rusqlite::Result<()> {
    let msg = &m.message;
    conn.execute(
        "INSERT INTO messages (id, thought_id, seq, role, kind, mode, display_text, model, context_ids, truncated, created_at, provider, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            msg.id,
            msg.thought_id,
            msg.seq,
            msg.role,
            msg.kind,
            msg.mode.as_str(),
            msg.text,
            msg.model,
            serde_json::to_string(&msg.context_ids).unwrap_or_else(|_| "[]".into()),
            msg.truncated,
            msg.created_at,
            m.provider,
            m.payload.to_string()
        ],
    )?;
    Ok(())
}

pub fn delete_message(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM messages WHERE id = ?1", [id])?;
    Ok(())
}

// --- Prompts ----------------------------------------------------------------

const PROMPT_COLUMNS: &str = "id, thought_id, message_id, content, edited, created_at, updated_at";

fn prompt_from_row(row: &Row) -> rusqlite::Result<Prompt> {
    Ok(Prompt {
        id: row.get(0)?,
        thought_id: row.get(1)?,
        message_id: row.get(2)?,
        content: row.get(3)?,
        edited: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn insert_prompt(conn: &Connection, p: &Prompt) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO prompts (id, thought_id, message_id, content, edited, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            p.id,
            p.thought_id,
            p.message_id,
            p.content,
            p.edited,
            p.created_at,
            p.updated_at
        ],
    )?;
    Ok(())
}

pub fn prompts_for(conn: &Connection, thought_id: &str) -> rusqlite::Result<Vec<Prompt>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROMPT_COLUMNS} FROM prompts WHERE thought_id = ?1 ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map([thought_id], prompt_from_row)?;
    rows.collect()
}

pub fn update_prompt(
    conn: &Connection,
    id: &str,
    content: &str,
    now: i64,
) -> rusqlite::Result<Option<Prompt>> {
    conn.execute(
        "UPDATE prompts SET content = ?2, edited = 1, updated_at = ?3 WHERE id = ?1",
        params![id, content, now],
    )?;
    conn.query_row(
        &format!("SELECT {PROMPT_COLUMNS} FROM prompts WHERE id = ?1"),
        [id],
        prompt_from_row,
    )
    .optional()
}

// --- Aggregate --------------------------------------------------------------

pub fn detail(conn: &Connection, id: &str) -> rusqlite::Result<Option<ThoughtDetail>> {
    let Some(thought) = get(conn, id)? else {
        return Ok(None);
    };
    let messages = load_messages(conn, id)?
        .into_iter()
        .map(|m| m.message)
        .collect();
    Ok(Some(ThoughtDetail {
        thought,
        messages,
        plans: plans::plans_for_thought(conn, id)?,
        tasks: plans::tasks_for_thought(conn, id)?,
        prompts: prompts_for(conn, id)?,
    }))
}

#[cfg(test)]
pub mod fixtures {
    use super::*;
    use crate::util::{new_id, now_ms};
    use serde_json::json;

    pub fn thought(conn: &Connection, title: &str, raw: &str) -> Thought {
        let now = now_ms();
        let t = Thought {
            id: new_id(),
            title: title.into(),
            raw_input: raw.into(),
            summary: String::new(),
            tags: vec![],
            mode: Mode::Think,
            include_in_memory: true,
            created_at: now,
            updated_at: now,
        };
        insert(conn, &t).unwrap();
        t
    }

    pub fn message(conn: &Connection, thought_id: &str, role: &str, text: &str) -> StoredMessage {
        let m = StoredMessage {
            message: Message {
                id: new_id(),
                thought_id: thought_id.into(),
                seq: next_seq(conn, thought_id).unwrap(),
                role: role.into(),
                kind: KIND_MESSAGE.into(),
                mode: Mode::Think,
                text: text.into(),
                model: None,
                context_ids: vec![],
                truncated: false,
                created_at: now_ms(),
            },
            provider: "anthropic".into(),
            payload: json!([{"type": "text", "text": text}]),
        };
        insert_message(conn, &m).unwrap();
        m
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures;
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn stores_and_replays_messages_in_order() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "Club", "philosophy club idea");
        fixtures::message(&conn, &t.id, "user", "first");
        fixtures::message(&conn, &t.id, "assistant", "second");
        let loaded = load_messages(&conn, &t.id).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].message.seq, 0);
        assert_eq!(loaded[1].message.role, "assistant");
        assert_eq!(loaded[1].payload[0]["text"], "second");
        assert_eq!(next_seq(&conn, &t.id).unwrap(), 2);
    }

    #[test]
    fn rename_tag_and_delete_cascade() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "Old", "raw");
        fixtures::message(&conn, &t.id, "user", "hello");
        rename(&conn, &t.id, "New", 1).unwrap();
        set_tags(&conn, &t.id, &["school".into()], 2).unwrap();
        let got = get(&conn, &t.id).unwrap().unwrap();
        assert_eq!(got.title, "New");
        assert_eq!(got.tags, vec!["school".to_string()]);
        assert!(rename(&conn, "missing", "x", 3).is_err());

        assert!(delete(&conn, &t.id).unwrap());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(get(&conn, &t.id).unwrap().is_none());
    }
}
