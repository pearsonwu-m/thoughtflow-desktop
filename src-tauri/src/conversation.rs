//! Conversation orchestration: builds each Claude request from local memory,
//! streams the reply, and persists the turn.
//!
//! History is append-only. A turn is stored only after Claude finishes, so a
//! cancelled or failed request leaves no half-written state behind, and stored
//! turns are replayed to the API exactly as they were first sent.

use crate::ai::anthropic::AnthropicClient;
use crate::ai::backend::Backend;
use crate::ai::claude_code::ClaudeCodeClient;
use crate::ai::prompts::{self, NoteContext};
use crate::ai::{AiError, ChatOptions, ChatResult, Effort, Mode, StreamEvent, Turn, TurnRole};
use crate::db::models::{
    Message, Plan, PlanStep, Prompt, StoredMessage, Task, Thought, ThoughtDetail,
};
use crate::db::{plans, search, thoughts};
use crate::error::{AppError, AppResult};
use crate::settings::{Connection, Settings};
use crate::state::{lock, AppState};
use crate::util::{derive_title, extract_prompt_block, new_id, now_ms, summarize_reply};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// Payload format of stored user and app turns (Anthropic-style content blocks).
const PROVIDER: &str = "anthropic";
const SUMMARY_CHARS: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SendKind {
    /// The user's own words.
    #[default]
    Message,
    /// ⌘K: ask Claude for its single most useful clarifying question.
    Clarify,
    /// A mode shortcut pressed with an empty composer: "turn this into a plan".
    ModeRequest,
}

impl SendKind {
    fn as_str(self) -> &'static str {
        match self {
            SendKind::Message => thoughts::KIND_MESSAGE,
            SendKind::Clarify => thoughts::KIND_CLARIFY,
            SendKind::ModeRequest => thoughts::KIND_MODE_REQUEST,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub request_id: String,
    pub thought_id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub kind: SendKind,
    /// Past thoughts the user chose to attach as context.
    #[serde(default)]
    pub context_ids: Vec<String>,
    /// The user's local date, e.g. "Thursday, October 1, 2026".
    #[serde(default)]
    pub local_date: String,
}

/// Where requests go: the API (needs a key) or the local Claude Code CLI.
async fn backend(state: &AppState, settings: &Settings) -> AppResult<Backend> {
    match settings.claude.connection {
        Connection::Api => {
            let (key, _) = state.keys.get().ok_or(AppError::MissingApiKey)?;
            Ok(Backend::Api(AnthropicClient::new(state.http.clone(), key)))
        }
        Connection::ClaudeCode => {
            let workdir = state.data_dir.join("claude-code");
            Ok(Backend::ClaudeCode(
                ClaudeCodeClient::locate(settings.claude.cli_path.clone(), workdir).await?,
            ))
        }
    }
}

fn chat_options(settings: &Settings) -> ChatOptions {
    ChatOptions {
        model: settings.claude.model.clone(),
        effort: settings.claude.effort,
        temperature: settings.claude.temperature,
        max_tokens: settings.claude.max_tokens,
    }
}

/// Rebuilds the provider turn for a stored message. Turns from another
/// provider (or with a missing payload) degrade to their visible text.
fn turn_from_stored(m: &StoredMessage) -> Turn {
    let role = TurnRole::parse(&m.message.role);
    let payload = if m.provider == PROVIDER && !m.payload.is_null() {
        m.payload.clone()
    } else if role == TurnRole::System {
        json!(m.message.text)
    } else {
        json!([{"type": "text", "text": m.message.text}])
    };
    Turn { role, payload }
}

fn not_found() -> AppError {
    AppError::NotFound("That thought no longer exists.".into())
}

fn stored(
    thought_id: &str,
    seq: i64,
    role: TurnRole,
    kind: &str,
    mode: Mode,
    text: &str,
    payload: Value,
) -> StoredMessage {
    StoredMessage {
        message: Message {
            id: new_id(),
            thought_id: thought_id.to_string(),
            seq,
            role: role.as_str().to_string(),
            kind: kind.to_string(),
            mode,
            text: text.to_string(),
            model: None,
            context_ids: Vec::new(),
            truncated: false,
            created_at: now_ms(),
        },
        provider: PROVIDER.to_string(),
        payload,
    }
}

/// Stores the reply and, in Prompt mode, the generated prompt it contains.
fn insert_reply(
    conn: &rusqlite::Connection,
    thought_id: &str,
    seq: i64,
    mode: Mode,
    provider: &str,
    result: &ChatResult,
    now: i64,
) -> rusqlite::Result<()> {
    let mut reply = stored(
        thought_id,
        seq,
        TurnRole::Assistant,
        thoughts::KIND_MESSAGE,
        mode,
        &result.text,
        result.content.clone(),
    );
    reply.message.created_at = now;
    reply.provider = provider.to_string();
    reply.message.model = Some(result.model.clone());
    reply.message.truncated = result.truncated();
    thoughts::insert_message(conn, &reply)?;
    if let Some(content) = extract_prompt_block(&result.text) {
        thoughts::insert_prompt(
            conn,
            &Prompt {
                id: new_id(),
                thought_id: thought_id.to_string(),
                message_id: Some(reply.message.id.clone()),
                content,
                edited: false,
                created_at: now,
                updated_at: now,
            },
        )?;
    }
    thoughts::touch(
        conn,
        thought_id,
        mode,
        Some(&summarize_reply(&result.text, SUMMARY_CHARS)),
        now,
    )
}

/// Streams a reply; returns it with the provider name to store alongside it.
async fn stream(
    state: &AppState,
    request_id: &str,
    turns: &[Turn],
    emit: &(dyn Fn(StreamEvent) + Send + Sync),
) -> AppResult<(ChatResult, &'static str)> {
    let settings = state.settings();
    let backend = backend(state, &settings).await?;
    let opts = chat_options(&settings);
    let token = state.begin_request(request_id);
    let result = backend.chat(&opts, turns, &token, emit).await;
    state.end_request(request_id);
    Ok((result?, backend.provider()))
}

/// Sends a user turn (creating the thought on its first reply) and returns
/// the updated thought.
pub async fn send(
    state: &AppState,
    req: SendRequest,
    emit: &(dyn Fn(StreamEvent) + Send + Sync),
) -> AppResult<ThoughtDetail> {
    let settings = state.settings();
    let user_text = match req.kind {
        SendKind::Message => req.text.trim().to_string(),
        SendKind::Clarify => prompts::CLARIFY_REQUEST.to_string(),
        SendKind::ModeRequest => prompts::mode_request(req.mode).to_string(),
    };
    if user_text.is_empty() {
        return Err(AppError::Invalid("Write a thought first.".into()));
    }
    // Fail fast, before touching storage, when there's no key.
    if settings.claude.connection == Connection::Api && state.keys.get().is_none() {
        return Err(AppError::MissingApiKey);
    }

    let (existing, history, notes): (Option<Thought>, Vec<StoredMessage>, Vec<NoteContext>) = {
        let conn = lock(&state.db);
        let existing = match &req.thought_id {
            Some(id) => Some(thoughts::get(&conn, id)?.ok_or_else(not_found)?),
            None => None,
        };
        let history = match &existing {
            Some(t) => thoughts::load_messages(&conn, &t.id)?,
            None => Vec::new(),
        };
        let notes = if settings.claude.use_memory && !req.context_ids.is_empty() {
            search::note_contexts(&conn, &req.context_ids)?
        } else {
            Vec::new()
        };
        (existing, history, notes)
    };

    let first_turn = history.is_empty();
    let mode_changed = first_turn || existing.as_ref().is_none_or(|t| t.mode != req.mode);
    let user_payload = prompts::user_content(&notes, &user_text);
    let mode_text =
        mode_changed.then(|| prompts::mode_message(req.mode, &req.local_date, first_turn));

    let mut turns: Vec<Turn> = history.iter().map(turn_from_stored).collect();
    turns.push(Turn {
        role: TurnRole::User,
        payload: user_payload.clone(),
    });
    if let Some(text) = &mode_text {
        turns.push(Turn {
            role: TurnRole::System,
            payload: json!(text),
        });
    }

    let (result, provider) = stream(state, &req.request_id, &turns, emit).await?;

    let now = now_ms();
    let mut conn = lock(&state.db);
    let tx = conn.transaction()?;
    let thought_id = match &existing {
        Some(t) => t.id.clone(),
        None => {
            let thought = Thought {
                id: new_id(),
                title: derive_title(&user_text),
                raw_input: user_text.clone(),
                summary: String::new(),
                tags: Vec::new(),
                mode: req.mode,
                include_in_memory: true,
                created_at: now,
                updated_at: now,
            };
            thoughts::insert(&tx, &thought)?;
            thought.id
        }
    };
    let mut seq = thoughts::next_seq(&tx, &thought_id)?;
    let mut user = stored(
        &thought_id,
        seq,
        TurnRole::User,
        req.kind.as_str(),
        req.mode,
        &user_text,
        user_payload,
    );
    user.message.created_at = now;
    if !notes.is_empty() {
        user.message.context_ids = req.context_ids.clone();
    }
    thoughts::insert_message(&tx, &user)?;
    seq += 1;
    if let Some(text) = &mode_text {
        let mut switch = stored(
            &thought_id,
            seq,
            TurnRole::System,
            thoughts::KIND_MODE_SWITCH,
            req.mode,
            "",
            json!(text),
        );
        switch.message.created_at = now;
        thoughts::insert_message(&tx, &switch)?;
        seq += 1;
    }
    insert_reply(&tx, &thought_id, seq, req.mode, provider, &result, now)?;
    tx.commit()?;
    thoughts::detail(&conn, &thought_id)?.ok_or_else(not_found)
}

/// Replaces the latest reply with a fresh one. Only the final turn is
/// removed, so every earlier turn is replayed unchanged.
pub async fn regenerate(
    state: &AppState,
    thought_id: &str,
    request_id: &str,
    emit: &(dyn Fn(StreamEvent) + Send + Sync),
) -> AppResult<ThoughtDetail> {
    let history = {
        let conn = lock(&state.db);
        thoughts::get(&conn, thought_id)?.ok_or_else(not_found)?;
        thoughts::load_messages(&conn, thought_id)?
    };
    let Some((last, prior)) = history
        .split_last()
        .filter(|(last, _)| last.message.role == "assistant")
    else {
        return Err(AppError::Invalid(
            "There's no reply to regenerate yet.".into(),
        ));
    };
    let turns: Vec<Turn> = prior.iter().map(turn_from_stored).collect();
    let (result, provider) = stream(state, request_id, &turns, emit).await?;

    let now = now_ms();
    let mut conn = lock(&state.db);
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM prompts WHERE message_id = ?1 AND edited = 0",
        [&last.message.id],
    )?;
    thoughts::delete_message(&tx, &last.message.id)?;
    insert_reply(
        &tx,
        thought_id,
        last.message.seq,
        last.message.mode,
        provider,
        &result,
        now,
    )?;
    tx.commit()?;
    thoughts::detail(&conn, thought_id)?.ok_or_else(not_found)
}

// ---------------------------------------------------------------------------
// Structured extraction
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct StepDraft {
    title: String,
    detail: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct PlanDraft {
    title: String,
    objective: String,
    why: String,
    steps: Vec<StepDraft>,
    next_action: String,
    obstacles: Vec<String>,
    deadline: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TaskDraft {
    name: String,
    description: String,
    priority: String,
    effort: String,
    due_date: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TasksDraft {
    tasks: Vec<TaskDraft>,
}

/// The conversation as plain text, for extraction. Only this thought is sent.
fn transcript_for(thought: &Thought, history: &[StoredMessage]) -> String {
    let mut turns: Vec<(String, String)> = history
        .iter()
        .filter(|m| m.message.role != "system" && !m.message.text.trim().is_empty())
        .map(|m| {
            let speaker = if m.message.role == "assistant" {
                "thoughtflow"
            } else {
                "user"
            };
            (speaker.to_string(), m.message.text.clone())
        })
        .collect();
    if turns.is_empty() {
        turns.push(("user".into(), thought.raw_input.clone()));
    }
    prompts::transcript(&turns)
}

async fn extract(
    state: &AppState,
    thought_id: &str,
    build: impl FnOnce(&str) -> (String, Value),
) -> AppResult<(Thought, Value)> {
    let settings = state.settings();
    let backend = backend(state, &settings).await?;
    let (thought, history) = {
        let conn = lock(&state.db);
        let thought = thoughts::get(&conn, thought_id)?.ok_or_else(not_found)?;
        let history = thoughts::load_messages(&conn, thought_id)?;
        (thought, history)
    };
    let (user_text, schema) = build(&transcript_for(&thought, &history));
    let opts = ChatOptions {
        model: settings.claude.model.clone(),
        effort: Effort::Low,
        temperature: None,
        max_tokens: 8_000,
    };
    let value = backend
        .extract(
            &opts,
            prompts::EXTRACTION_SYSTEM_PROMPT,
            &user_text,
            &schema,
            &CancellationToken::new(),
        )
        .await?;
    Ok((thought, value))
}

fn malformed() -> AppError {
    AppError::Ai(AiError::Malformed("unexpected structure".into()))
}

/// Asks Claude for a structured version of the plan discussed in a thought
/// and saves it.
pub async fn extract_plan(state: &AppState, thought_id: &str, today: &str) -> AppResult<Plan> {
    let (thought, value) = extract(state, thought_id, |transcript| {
        (
            prompts::plan_extraction_request(today, transcript),
            prompts::plan_schema(),
        )
    })
    .await?;
    let draft: PlanDraft = serde_json::from_value(value).map_err(|_| malformed())?;
    if draft.objective.trim().is_empty() && draft.steps.is_empty() {
        return Err(malformed());
    }
    let now = now_ms();
    let plan = plans::normalize_plan(Plan {
        id: new_id(),
        thought_id: Some(thought.id.clone()),
        title: if draft.title.trim().is_empty() {
            thought.title.clone()
        } else {
            draft.title
        },
        objective: draft.objective.trim().to_string(),
        why: draft.why.trim().to_string(),
        steps: draft
            .steps
            .into_iter()
            .map(|s| PlanStep {
                title: s.title.trim().to_string(),
                detail: s.detail.trim().to_string(),
                done: false,
            })
            .collect(),
        next_action: draft.next_action.trim().to_string(),
        obstacles: draft
            .obstacles
            .into_iter()
            .map(|o| o.trim().to_string())
            .collect(),
        deadline: draft.deadline,
        status: "active".into(),
        created_at: now,
        updated_at: now,
    });
    let conn = lock(&state.db);
    plans::insert_plan(&conn, &plan)?;
    Ok(plan)
}

/// Asks Claude for the concrete tasks in a thought and saves them.
pub async fn extract_tasks(
    state: &AppState,
    thought_id: &str,
    today: &str,
) -> AppResult<Vec<Task>> {
    let (thought, value) = extract(state, thought_id, |transcript| {
        (
            prompts::tasks_extraction_request(today, transcript),
            prompts::tasks_schema(),
        )
    })
    .await?;
    let draft: TasksDraft = serde_json::from_value(value).map_err(|_| malformed())?;
    let now = now_ms();
    let tasks: Vec<Task> = draft
        .tasks
        .into_iter()
        .filter(|t| !t.name.trim().is_empty())
        .take(30)
        .map(|t| {
            plans::normalize_task(Task {
                id: new_id(),
                thought_id: Some(thought.id.clone()),
                plan_id: None,
                name: t.name,
                description: t.description,
                priority: t.priority,
                effort: t.effort,
                due_date: t.due_date,
                completed: false,
                completed_at: None,
                created_at: now,
                updated_at: now,
            })
        })
        .collect();
    if tasks.is_empty() {
        return Err(AppError::Invalid(
            "Claude didn't find concrete tasks in this thought yet.".into(),
        ));
    }
    let mut conn = lock(&state.db);
    let tx = conn.transaction()?;
    for task in &tasks {
        plans::insert_task(&tx, task)?;
    }
    tx.commit()?;
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::thoughts::fixtures;

    /// App state wired to a fake `claude` executable that records its stdin
    /// and replies with `reply`.
    fn claude_code_state(dir: &std::path::Path, reply: &str) -> AppState {
        use std::os::unix::fs::PermissionsExt;
        let program = dir.join("claude");
        let result = json!({"type": "result", "subtype": "success", "is_error": false, "result": reply, "stop_reason": "end_turn"});
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\ncat > \"{}/stdin.txt\"\ncat <<'TF_EOF'\n{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"claude-opus-5-5\"}}\n{result}\nTF_EOF\n",
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut settings = Settings::default();
        settings.claude.connection = Connection::ClaudeCode;
        settings.claude.cli_path = Some(program.display().to_string());
        AppState {
            db: std::sync::Mutex::new(open_in_memory()),
            db_path: dir.join("thoughtflow.db"),
            data_dir: dir.to_path_buf(),
            settings: std::sync::Mutex::new(settings),
            settings_path: dir.join("settings.json"),
            keys: Default::default(),
            http: reqwest::Client::new(),
            inflight: Default::default(),
            shortcut: Default::default(),
            pending_settings_section: Default::default(),
            startup_warning: Default::default(),
        }
    }

    #[tokio::test]
    async fn claude_code_conversations_are_saved_as_plain_text() {
        let dir = std::env::temp_dir().join(format!("tf-conv-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = claude_code_state(&dir, "What I'm hearing: two deadlines.");
        let request = |thought_id: Option<String>, text: &str| SendRequest {
            request_id: new_id(),
            thought_id,
            text: text.into(),
            mode: Mode::Think,
            kind: SendKind::Message,
            context_ids: vec![],
            local_date: "Friday, October 2, 2026".into(),
        };

        let detail = send(
            &state,
            request(None, "finish physics and email Dr. Lignos"),
            &|_| {},
        )
        .await
        .unwrap();
        let roles: Vec<&str> = detail.messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "system", "assistant"]);
        assert_eq!(detail.messages[2].text, "What I'm hearing: two deadlines.");
        let stored = thoughts::load_messages(&lock(&state.db), &detail.thought.id).unwrap();
        assert_eq!(stored[2].provider, "claude-code");
        assert_eq!(
            stored[2].payload,
            json!([{"type": "text", "text": "What I'm hearing: two deadlines."}])
        );
        let first_prompt = std::fs::read_to_string(dir.join("stdin.txt")).unwrap();
        assert!(first_prompt.contains("Today is Friday, October 2, 2026."));
        assert!(first_prompt.ends_with("finish physics and email Dr. Lignos"));

        // The follow-up carries the earlier exchange as a transcript.
        send(
            &state,
            request(Some(detail.thought.id.clone()), "Physics is due Monday"),
            &|_| {},
        )
        .await
        .unwrap();
        let second_prompt = std::fs::read_to_string(dir.join("stdin.txt")).unwrap();
        assert!(second_prompt
            .contains("<thoughtflow>\nWhat I'm hearing: two deadlines.\n</thoughtflow>"));
        assert!(second_prompt.ends_with("Physics is due Monday"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn foreign_turns_degrade_to_visible_text() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "x", "y");
        let mut m = fixtures::message(&conn, &t.id, "assistant", "hello");
        assert_eq!(
            turn_from_stored(&m).payload,
            json!([{"type": "text", "text": "hello"}])
        );
        m.provider = "other".into();
        m.payload = json!({"weird": true});
        assert_eq!(
            turn_from_stored(&m).payload,
            json!([{"type": "text", "text": "hello"}])
        );
    }

    #[test]
    fn transcript_skips_app_messages_and_falls_back_to_raw_input() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "x", "raw thought");
        assert!(transcript_for(&t, &[]).contains("raw thought"));
        let mut history = vec![fixtures::message(&conn, &t.id, "user", "hi")];
        let mut system = fixtures::message(&conn, &t.id, "system", "");
        system.message.role = "system".into();
        history.push(system);
        history.push(fixtures::message(&conn, &t.id, "assistant", "hello"));
        let transcript = transcript_for(&t, &history);
        assert_eq!(
            transcript,
            "<user>\nhi\n</user>\n<thoughtflow>\nhello\n</thoughtflow>\n"
        );
    }

    #[test]
    fn insert_reply_saves_generated_prompts() {
        let conn = open_in_memory();
        let t = fixtures::thought(&conn, "x", "y");
        let result = ChatResult {
            model: "claude-opus-5-5".into(),
            content: json!([{"type": "text", "text": "<prompt>You are a tutor.</prompt>"}]),
            text: "<prompt>You are a tutor.</prompt>".into(),
            stop_reason: "end_turn".into(),
        };
        insert_reply(&conn, &t.id, 0, Mode::Prompt, "claude-code", &result, 5).unwrap();
        let prompts = thoughts::prompts_for(&conn, &t.id).unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].content, "You are a tutor.");
        assert_eq!(
            thoughts::get(&conn, &t.id).unwrap().unwrap().mode,
            Mode::Prompt
        );
    }
}
