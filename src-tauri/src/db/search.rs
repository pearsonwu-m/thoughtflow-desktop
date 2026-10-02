//! History search and related-memory retrieval.
//!
//! Retrieval is deliberately simple: SQLite FTS5 with BM25 ranking over
//! titles, raw input, summaries, and tags. Only the few most relevant notes
//! the user hasn't excluded are offered, and the user sees (and can remove)
//! each one before anything is sent.

use super::models::{RelatedNote, Thought};
use super::thoughts::{thought_from_row, THOUGHT_COLUMNS};
use crate::ai::prompts::NoteContext;
use crate::util::{truncate_chars, ymd_utc};
use rusqlite::{params, Connection};

const STOPWORDS: &[&str] = &[
    "a", "about", "after", "again", "all", "also", "am", "an", "and", "any", "are", "as", "at",
    "be", "been", "but", "by", "can", "could", "did", "do", "does", "doing", "don", "dont", "for",
    "from", "get", "go", "going", "got", "had", "has", "have", "how", "i", "im", "if", "in",
    "into", "is", "it", "its", "just", "know", "like", "maybe", "me", "more", "my", "need", "not",
    "now", "of", "on", "or", "our", "out", "really", "should", "so", "some", "still", "that",
    "the", "their", "them", "then", "there", "these", "they", "thing", "things", "think",
    "thinking", "this", "to", "too", "up", "very", "want", "was", "we", "what", "when", "which",
    "while", "who", "why", "will", "with", "would", "you", "your", "ive", "id", "ill", "actually",
    "been", "being", "much", "work", "way", "make", "lot",
];

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Every token must match (prefix match on the last one, for search-as-you-type).
pub fn fts_all_terms(query: &str) -> Option<String> {
    let toks = tokens(query);
    if toks.is_empty() {
        return None;
    }
    let last = toks.len() - 1;
    Some(
        toks.iter()
            .enumerate()
            .map(|(i, t)| {
                if i == last {
                    format!("\"{t}\"*")
                } else {
                    format!("\"{t}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Any meaningful token may match; used to find related notes.
pub fn fts_any_terms(text: &str) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    for t in tokens(text) {
        if t.chars().count() < 3
            || STOPWORDS.contains(&t.as_str())
            || t.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        if !seen.contains(&t) {
            seen.push(t);
        }
        if seen.len() == 16 {
            break;
        }
    }
    if seen.is_empty() {
        return None;
    }
    Some(
        seen.iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// Searches titles, raw thoughts, summaries, tags, and conversation text.
pub fn search_thoughts(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<Thought>> {
    if query.trim().is_empty() {
        return super::thoughts::list_recent(conn, limit);
    }
    let mut results = Vec::new();
    if let Some(fts) = fts_all_terms(query) {
        let sql = format!(
            "WITH hits AS (
                SELECT t.id AS id, bm25(thoughts_fts, 10.0, 4.0, 2.0, 6.0) AS score
                FROM thoughts_fts JOIN thoughts t ON t.rowid = thoughts_fts.rowid
                WHERE thoughts_fts MATCH ?1
                UNION ALL
                SELECT m.thought_id AS id, bm25(messages_fts) + 1.0 AS score
                FROM messages_fts JOIN messages m ON m.rowid = messages_fts.rowid
                WHERE messages_fts MATCH ?1 AND m.role != 'system'
            )
            SELECT {cols} FROM thoughts
            JOIN (SELECT id AS hit_id, MIN(score) AS score FROM hits GROUP BY id) h ON h.hit_id = thoughts.id
            ORDER BY h.score ASC, thoughts.updated_at DESC
            LIMIT ?2",
            cols = THOUGHT_COLUMNS
                .split(", ")
                .map(|c| format!("thoughts.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![fts, limit], thought_from_row)?;
        results = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    }
    // Substring fallback covers scripts the tokenizer doesn't segment
    // (e.g. Chinese) and partial words in the middle of a token.
    if results.is_empty() {
        let mut stmt = conn.prepare(&format!(
            "SELECT {THOUGHT_COLUMNS} FROM thoughts t
             WHERE t.title LIKE ?1 ESCAPE '\\' OR t.raw_input LIKE ?1 ESCAPE '\\'
                OR t.tags LIKE ?1 ESCAPE '\\' OR t.summary LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM messages m WHERE m.thought_id = t.id AND m.display_text LIKE ?1 ESCAPE '\\')
             ORDER BY t.updated_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![like_pattern(query), limit], thought_from_row)?;
        results = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(results)
}

fn snippet(t: &Thought, max: usize) -> String {
    let source = if t.summary.trim().is_empty() {
        &t.raw_input
    } else {
        &t.summary
    };
    truncate_chars(source, max)
}

/// Past thoughts most related to `text`, excluding `exclude_id` and anything
/// the user removed from memory.
pub fn related_notes(
    conn: &Connection,
    text: &str,
    exclude_id: Option<&str>,
    limit: u32,
) -> rusqlite::Result<Vec<RelatedNote>> {
    let Some(fts) = fts_any_terms(text) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {cols} FROM thoughts_fts JOIN thoughts t ON t.rowid = thoughts_fts.rowid
         WHERE thoughts_fts MATCH ?1 AND t.include_in_memory = 1 AND t.id != ?2
         ORDER BY bm25(thoughts_fts, 10.0, 4.0, 2.0, 6.0) ASC
         LIMIT ?3",
        cols = THOUGHT_COLUMNS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows = stmt.query_map(
        params![fts, exclude_id.unwrap_or(""), limit],
        thought_from_row,
    )?;
    rows.map(|r| {
        r.map(|t| RelatedNote {
            snippet: snippet(&t, 140),
            id: t.id,
            title: t.title,
            created_at: t.created_at,
        })
    })
    .collect()
}

/// The context sent to Claude for notes the user chose to attach.
pub fn note_contexts(conn: &Connection, ids: &[String]) -> rusqlite::Result<Vec<NoteContext>> {
    let mut out = Vec::new();
    for id in ids.iter().take(5) {
        let Some(t) = super::thoughts::get(conn, id)? else {
            continue;
        };
        if !t.include_in_memory {
            continue;
        }
        let mut excerpt = format!("The user wrote: {}", truncate_chars(&t.raw_input, 500));
        if !t.summary.trim().is_empty() {
            excerpt.push_str(&format!("\nTakeaway: {}", truncate_chars(&t.summary, 300)));
        }
        out.push(NoteContext {
            title: t.title,
            date: ymd_utc(t.created_at),
            tags: t.tags,
            excerpt,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::thoughts::{self, fixtures};

    #[test]
    fn builds_safe_fts_queries() {
        assert_eq!(
            fts_all_terms("physics stu").unwrap(),
            "\"physics\" \"stu\"*"
        );
        assert_eq!(
            fts_all_terms("\"; DROP TABLE"),
            Some("\"drop\" \"table\"*".into())
        );
        assert_eq!(fts_all_terms("  !!  "), None);
        assert_eq!(
            fts_any_terms("I need to finish my physics homework and email Dr. Lignos").unwrap(),
            "\"finish\" OR \"physics\" OR \"homework\" OR \"email\" OR \"lignos\""
        );
        assert_eq!(fts_any_terms("I am so so"), None);
    }

    #[test]
    fn searches_titles_conversations_and_cjk_text() {
        let conn = open_in_memory();
        let physics = fixtures::thought(&conn, "Physics study plan", "finish problem set three");
        let club = fixtures::thought(&conn, "Philosophy club", "start a club at school");
        fixtures::message(
            &conn,
            &club.id,
            "assistant",
            "Consider a reading group on Foucault",
        );
        let cjk = fixtures::thought(&conn, "键盘政治", "研究中国互联网上的键盘政治");

        let ids = |q: &str| {
            search_thoughts(&conn, q, 10)
                .unwrap()
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("physics"), vec![physics.id.clone()]);
        assert_eq!(
            ids("phys"),
            vec![physics.id.clone()],
            "prefix match while typing"
        );
        assert_eq!(
            ids("foucault"),
            vec![club.id.clone()],
            "matches conversation text"
        );
        assert_eq!(
            ids("互联网"),
            vec![cjk.id.clone()],
            "substring fallback for CJK"
        );
        assert_eq!(ids("").len(), 3);
        assert!(ids("nonexistentword").is_empty());
    }

    #[test]
    fn related_notes_respect_memory_exclusions() {
        let conn = open_in_memory();
        let a = fixtures::thought(
            &conn,
            "Physics exam prep",
            "physics exam on friday, need to review optics",
        );
        let b = fixtures::thought(&conn, "Private physics worry", "physics grade anxiety");
        let current = fixtures::thought(&conn, "Now", "physics homework");
        thoughts::set_include_in_memory(&conn, &b.id, false).unwrap();

        let related = related_notes(
            &conn,
            "I should review optics for physics",
            Some(&current.id),
            3,
        )
        .unwrap();
        let ids: Vec<_> = related.iter().map(|n| n.id.clone()).collect();
        assert_eq!(ids, vec![a.id.clone()]);

        let contexts = note_contexts(&conn, &[a.id.clone(), b.id.clone()]).unwrap();
        assert_eq!(contexts.len(), 1, "excluded notes are never sent");
        assert!(contexts[0].excerpt.contains("review optics"));
    }
}
