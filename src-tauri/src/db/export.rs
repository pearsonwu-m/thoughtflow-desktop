//! Data export and bulk deletion.

use super::{plans, thoughts};
use rusqlite::Connection;
use serde_json::{json, Value};

/// Everything the user has stored, as readable JSON. Provider wire payloads
/// (which include opaque reasoning signatures) are omitted; the visible text
/// of every turn is included.
pub fn export_all(conn: &Connection, exported_at: i64) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare("SELECT id FROM thoughts ORDER BY created_at ASC")?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(thought) = thoughts::get(conn, &id)? else {
            continue;
        };
        let messages: Vec<Value> = thoughts::load_messages(conn, &id)?
            .into_iter()
            .filter(|m| m.message.role != "system")
            .map(|m| {
                json!({
                    "role": m.message.role,
                    "kind": m.message.kind,
                    "mode": m.message.mode,
                    "text": m.message.text,
                    "model": m.message.model,
                    "createdAt": m.message.created_at,
                })
            })
            .collect();
        items.push(json!({
            "thought": thought,
            "conversation": messages,
            "prompts": thoughts::prompts_for(conn, &id)?,
        }));
    }
    Ok(json!({
        "app": "Thoughtflow",
        "formatVersion": 1,
        "exportedAt": exported_at,
        "thoughts": items,
        "plans": plans::list_plans(conn)?,
        "tasks": plans::list_tasks(conn)?,
    }))
}

/// Removes every thought, conversation, plan, task, and prompt, then compacts
/// the file so deleted content doesn't remain on disk.
pub fn clear_all(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "BEGIN;
         DELETE FROM tasks;
         DELETE FROM plans;
         DELETE FROM prompts;
         DELETE FROM messages;
         DELETE FROM thoughts;
         INSERT INTO thoughts_fts(thoughts_fts) VALUES ('rebuild');
         INSERT INTO messages_fts(messages_fts) VALUES ('rebuild');
         COMMIT;",
    )?;
    super::checkpoint(conn)?;
    conn.execute_batch("VACUUM;")?;
    super::checkpoint(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::thoughts::fixtures;

    #[test]
    fn exports_visible_text_and_clears_everything() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "Website", "work on my website");
        fixtures::message(&conn, &t.id, "user", "work on my website");
        fixtures::message(&conn, &t.id, "assistant", "What's blocking you?");

        let export = export_all(&conn, 42).unwrap();
        assert_eq!(export["thoughts"][0]["thought"]["title"], "Website");
        assert_eq!(
            export["thoughts"][0]["conversation"][1]["text"],
            "What's blocking you?"
        );
        assert!(export.to_string().find("payload").is_none());

        clear_all(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thoughts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(crate::db::search::search_thoughts(&conn, "website", 10)
            .unwrap()
            .is_empty());
    }
}
