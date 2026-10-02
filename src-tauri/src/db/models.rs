//! Records shared between storage, commands, and the UI. Field names are
//! camelCased on the wire to match `src/types.ts`.

use crate::ai::Mode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thought {
    pub id: String,
    pub title: String,
    pub raw_input: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub mode: Mode,
    pub include_in_memory: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A conversation turn as the UI sees it (no provider payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub thought_id: String,
    pub seq: i64,
    pub role: String,
    /// `message`, `clarify`, `modeRequest`, or `modeSwitch`.
    pub kind: String,
    pub mode: Mode,
    pub text: String,
    pub model: Option<String>,
    pub context_ids: Vec<String>,
    pub truncated: bool,
    pub created_at: i64,
}

/// A turn plus the provider wire content that is replayed to the API.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub message: Message,
    pub provider: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub thought_id: String,
    pub message_id: Option<String>,
    pub content: String,
    pub edited: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub thought_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub why: String,
    pub steps: Vec<PlanStep>,
    pub next_action: String,
    pub obstacles: Vec<String>,
    pub deadline: Option<String>,
    /// `active`, `done`, or `archived`.
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub thought_id: Option<String>,
    pub plan_id: Option<String>,
    pub name: String,
    pub description: String,
    /// `high`, `medium`, or `low`.
    pub priority: String,
    /// `quick`, `short`, `medium`, or `long`.
    pub effort: String,
    pub due_date: Option<String>,
    pub completed: bool,
    pub completed_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThoughtDetail {
    pub thought: Thought,
    pub messages: Vec<Message>,
    pub plans: Vec<Plan>,
    pub tasks: Vec<Task>,
    pub prompts: Vec<Prompt>,
}

/// A past thought offered as context for a new one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedNote {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub snippet: String,
}

pub fn normalize_priority(p: &str) -> String {
    match p.trim().to_ascii_lowercase().as_str() {
        "high" => "high",
        "low" => "low",
        _ => "medium",
    }
    .to_string()
}

pub fn normalize_effort(e: &str) -> String {
    match e.trim().to_ascii_lowercase().as_str() {
        "quick" => "quick",
        "medium" => "medium",
        "long" => "long",
        _ => "short",
    }
    .to_string()
}

pub fn normalize_status(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "done" => "done",
        "archived" => "archived",
        _ => "active",
    }
    .to_string()
}

/// Accepts only `YYYY-MM-DD`; anything else becomes `None`.
pub fn normalize_date(d: Option<&str>) -> Option<String> {
    let d = d?.trim();
    let bytes = d.as_bytes();
    let ok = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    ok.then(|| d.to_string())
}

pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let t = tag.trim().trim_start_matches('#').trim().to_lowercase();
        if !t.is_empty() && t.chars().count() <= 32 && !out.contains(&t) {
            out.push(t);
        }
    }
    out.truncate(12);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizers_reject_junk() {
        assert_eq!(normalize_priority("HIGH"), "high");
        assert_eq!(normalize_priority("urgent"), "medium");
        assert_eq!(normalize_effort("long"), "long");
        assert_eq!(normalize_status("whatever"), "active");
        assert_eq!(
            normalize_date(Some("2026-10-02")),
            Some("2026-10-02".into())
        );
        assert_eq!(normalize_date(Some("next friday")), None);
        assert_eq!(normalize_date(None), None);
        assert_eq!(
            normalize_tags(&[
                "#School".into(),
                "school".into(),
                " ".into(),
                "Ideas".into()
            ]),
            vec!["school".to_string(), "ideas".to_string()]
        );
    }
}
