//! Tauri commands: the only surface the webview can call.
//!
//! Commands are thin. They validate input, take the lock they need, and
//! delegate to `conversation`, `db`, `secrets`, or `desktop`.

use crate::ai::anthropic::AnthropicClient;
use crate::ai::{Mode, ModelInfo, StreamEvent};
use crate::conversation::{self, SendRequest};
use crate::db::models::{normalize_tags, Plan, Prompt, RelatedNote, Task, Thought, ThoughtDetail};
use crate::db::{self, export, plans, search, thoughts};
use crate::desktop::{shortcut, window};
use crate::error::{AppError, AppResult};
use crate::secrets::ApiKeyStatus;
use crate::settings::Settings;
use crate::state::{lock, AppState, ShortcutStatus};
use crate::util::{derive_title, new_id, now_ms};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

fn missing(e: rusqlite::Error) -> AppError {
    match e {
        rusqlite::Error::QueryReturnedNoRows => {
            AppError::NotFound("That item no longer exists.".into())
        }
        other => AppError::Database(other),
    }
}

// --- App & settings -----------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    db_path: String,
    shortcut: ShortcutStatus,
    api_key: ApiKeyStatus,
    startup_warning: Option<String>,
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.data_dir.display().to_string(),
        db_path: state.db_path.display().to_string(),
        shortcut: lock(&state.shortcut).clone(),
        api_key: state.keys.status(),
        startup_warning: lock(&state.startup_warning).clone(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// Saves settings and applies their side effects (global shortcut, launch at
/// login). Every window is told so it can refresh theme and bindings.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    let previous = state.settings();
    let mut next = settings.sanitized();
    // The widget position is owned by the backend.
    next.widget = previous.widget;

    if next.general.global_shortcut != previous.general.global_shortcut {
        let status = shortcut::register(&app, &next.general.global_shortcut);
        if !status.registered {
            shortcut::register(&app, &previous.general.global_shortcut);
            return Err(AppError::Invalid(status.error.unwrap_or_default()));
        }
    }
    if next.general.launch_at_login != previous.general.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if next.general.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if result.is_err() {
            return Err(AppError::Invalid(
                "Couldn't change the launch-at-login setting.".into(),
            ));
        }
    }
    let saved = state.save_settings(next)?;
    let _ = app.emit("tf://settings-changed", saved.clone());
    Ok(saved)
}

#[tauri::command]
pub fn get_shortcut_status(state: State<'_, AppState>) -> ShortcutStatus {
    lock(&state.shortcut).clone()
}

// --- API key ------------------------------------------------------------------

#[tauri::command]
pub fn get_api_key_status(state: State<'_, AppState>) -> ApiKeyStatus {
    state.keys.status()
}

#[tauri::command]
pub fn set_api_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> AppResult<ApiKeyStatus> {
    state.keys.set(&key).map_err(AppError::Keychain)?;
    let status = state.keys.status();
    let _ = app.emit("tf://api-key-changed", status.clone());
    Ok(status)
}

#[tauri::command]
pub fn delete_api_key(app: AppHandle, state: State<'_, AppState>) -> AppResult<ApiKeyStatus> {
    state.keys.delete().map_err(AppError::Keychain)?;
    let status = state.keys.status();
    let _ = app.emit("tf://api-key-changed", status.clone());
    Ok(status)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    latency_ms: u64,
    models: Vec<ModelInfo>,
    model_available: bool,
}

/// Verifies the key by listing models (no tokens are spent and no thought
/// content is sent).
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> AppResult<ConnectionTest> {
    let (key, _) = state.keys.get().ok_or(AppError::MissingApiKey)?;
    let model = state.settings().claude.model;
    let started = Instant::now();
    let models = AnthropicClient::new(state.http.clone(), key)
        .list_models()
        .await?;
    Ok(ConnectionTest {
        latency_ms: started.elapsed().as_millis() as u64,
        model_available: models.iter().any(|m| m.id == model),
        models,
    })
}

// --- Windows ------------------------------------------------------------------

#[tauri::command]
pub fn show_widget(app: AppHandle, view: Option<String>) {
    window::show_widget(&app, view.as_deref());
}

#[tauri::command]
pub fn hide_widget(app: AppHandle) {
    window::hide_widget(&app);
}

#[tauri::command]
pub fn resize_widget(app: AppHandle, height: f64) -> AppResult<()> {
    if let Some(w) = window::widget(&app) {
        window::resize_widget(&w, height).map_err(|e| AppError::Internal(e.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_settings(app: AppHandle, section: Option<String>) {
    window::open_settings(&app, section.as_deref());
}

#[tauri::command]
pub fn take_settings_section(state: State<'_, AppState>) -> Option<String> {
    lock(&state.pending_settings_section).take()
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::desktop::tray::quit(&app);
}

// --- Thoughts -------------------------------------------------------------------

#[tauri::command]
pub fn list_thoughts(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<u32>,
) -> AppResult<Vec<Thought>> {
    let conn = lock(&state.db);
    let limit = limit.unwrap_or(200).min(1_000);
    Ok(search::search_thoughts(
        &conn,
        query.as_deref().unwrap_or(""),
        limit,
    )?)
}

#[tauri::command]
pub fn get_thought(state: State<'_, AppState>, id: String) -> AppResult<ThoughtDetail> {
    let conn = lock(&state.db);
    thoughts::detail(&conn, &id)?
        .ok_or_else(|| AppError::NotFound("That thought no longer exists.".into()))
}

/// Saves a thought locally without sending anything to Claude.
#[tauri::command]
pub fn capture_thought(
    state: State<'_, AppState>,
    text: String,
    mode: Option<Mode>,
) -> AppResult<ThoughtDetail> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::Invalid("Write a thought first.".into()));
    }
    let now = now_ms();
    let thought = Thought {
        id: new_id(),
        title: derive_title(text),
        raw_input: text.to_string(),
        summary: String::new(),
        tags: Vec::new(),
        mode: mode.unwrap_or_default(),
        include_in_memory: true,
        created_at: now,
        updated_at: now,
    };
    let conn = lock(&state.db);
    thoughts::insert(&conn, &thought)?;
    thoughts::detail(&conn, &thought.id)?
        .ok_or_else(|| AppError::Internal("Saved thought vanished.".into()))
}

#[tauri::command]
pub fn rename_thought(state: State<'_, AppState>, id: String, title: String) -> AppResult<Thought> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Invalid("A title can't be empty.".into()));
    }
    let conn = lock(&state.db);
    thoughts::rename(
        &conn,
        &id,
        &crate::util::truncate_chars(title, 120),
        now_ms(),
    )
    .map_err(missing)?;
    thoughts::get(&conn, &id)?
        .ok_or_else(|| AppError::NotFound("That thought no longer exists.".into()))
}

#[tauri::command]
pub fn set_thought_tags(
    state: State<'_, AppState>,
    id: String,
    tags: Vec<String>,
) -> AppResult<Thought> {
    let conn = lock(&state.db);
    thoughts::set_tags(&conn, &id, &normalize_tags(&tags), now_ms()).map_err(missing)?;
    thoughts::get(&conn, &id)?
        .ok_or_else(|| AppError::NotFound("That thought no longer exists.".into()))
}

/// Includes or excludes a thought from the memory Claude can be offered.
#[tauri::command]
pub fn set_thought_memory(
    state: State<'_, AppState>,
    id: String,
    include: bool,
) -> AppResult<Thought> {
    let conn = lock(&state.db);
    thoughts::set_include_in_memory(&conn, &id, include).map_err(missing)?;
    thoughts::get(&conn, &id)?
        .ok_or_else(|| AppError::NotFound("That thought no longer exists.".into()))
}

/// Permanently deletes a thought and everything made from it.
#[tauri::command]
pub fn delete_thought(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    let conn = lock(&state.db);
    let deleted = thoughts::delete(&conn, &id)?;
    db::checkpoint(&conn)?;
    Ok(deleted)
}

#[tauri::command]
pub fn find_related(
    state: State<'_, AppState>,
    text: String,
    exclude_id: Option<String>,
) -> AppResult<Vec<RelatedNote>> {
    if !state.settings().claude.use_memory {
        return Ok(Vec::new());
    }
    let conn = lock(&state.db);
    Ok(search::related_notes(
        &conn,
        &text,
        exclude_id.as_deref(),
        3,
    )?)
}

// --- Claude conversation ------------------------------------------------------------

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    request: SendRequest,
    on_event: Channel<StreamEvent>,
) -> AppResult<ThoughtDetail> {
    let emit = move |event: StreamEvent| {
        let _ = on_event.send(event);
    };
    conversation::send(&state, request, &emit).await
}

#[tauri::command]
pub async fn regenerate_response(
    state: State<'_, AppState>,
    thought_id: String,
    request_id: String,
    on_event: Channel<StreamEvent>,
) -> AppResult<ThoughtDetail> {
    let emit = move |event: StreamEvent| {
        let _ = on_event.send(event);
    };
    conversation::regenerate(&state, &thought_id, &request_id, &emit).await
}

#[tauri::command]
pub fn cancel_message(state: State<'_, AppState>, request_id: String) -> bool {
    state.cancel_request(&request_id)
}

#[tauri::command]
pub async fn extract_plan(
    state: State<'_, AppState>,
    thought_id: String,
    today: String,
) -> AppResult<Plan> {
    conversation::extract_plan(&state, &thought_id, &today).await
}

#[tauri::command]
pub async fn extract_tasks(
    state: State<'_, AppState>,
    thought_id: String,
    today: String,
) -> AppResult<Vec<Task>> {
    conversation::extract_tasks(&state, &thought_id, &today).await
}

#[tauri::command]
pub fn update_prompt(state: State<'_, AppState>, id: String, content: String) -> AppResult<Prompt> {
    let content = content.trim();
    if content.is_empty() {
        return Err(AppError::Invalid("A prompt can't be empty.".into()));
    }
    let conn = lock(&state.db);
    thoughts::update_prompt(&conn, &id, content, now_ms())?
        .ok_or_else(|| AppError::NotFound("That prompt no longer exists.".into()))
}

// --- Plans & tasks ------------------------------------------------------------------

#[tauri::command]
pub fn list_plans(state: State<'_, AppState>) -> AppResult<Vec<Plan>> {
    Ok(plans::list_plans(&lock(&state.db))?)
}

#[tauri::command]
pub fn update_plan(state: State<'_, AppState>, plan: Plan) -> AppResult<Plan> {
    let mut plan = plans::normalize_plan(plan);
    plan.updated_at = now_ms();
    let conn = lock(&state.db);
    plans::update_plan(&conn, &plan).map_err(missing)?;
    plans::get_plan(&conn, &plan.id)?
        .ok_or_else(|| AppError::NotFound("That plan no longer exists.".into()))
}

#[tauri::command]
pub fn delete_plan(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    Ok(plans::delete_plan(&lock(&state.db), &id)?)
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> AppResult<Vec<Task>> {
    Ok(plans::list_tasks(&lock(&state.db))?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    effort: String,
    due_date: Option<String>,
    thought_id: Option<String>,
}

#[tauri::command]
pub fn create_task(state: State<'_, AppState>, task: NewTask) -> AppResult<Task> {
    if task.name.trim().is_empty() {
        return Err(AppError::Invalid("A task needs a name.".into()));
    }
    let now = now_ms();
    let task = plans::normalize_task(Task {
        id: new_id(),
        thought_id: task.thought_id,
        plan_id: None,
        name: task.name,
        description: task.description,
        priority: task.priority,
        effort: task.effort,
        due_date: task.due_date,
        completed: false,
        completed_at: None,
        created_at: now,
        updated_at: now,
    });
    plans::insert_task(&lock(&state.db), &task)?;
    Ok(task)
}

#[tauri::command]
pub fn update_task(state: State<'_, AppState>, task: Task) -> AppResult<Task> {
    let now = now_ms();
    let mut task = plans::normalize_task(task);
    if task.name.is_empty() {
        return Err(AppError::Invalid("A task needs a name.".into()));
    }
    task.completed_at = match (task.completed, task.completed_at) {
        (true, None) => Some(now),
        (true, at) => at,
        (false, _) => None,
    };
    task.updated_at = now;
    let conn = lock(&state.db);
    plans::update_task(&conn, &task).map_err(missing)?;
    plans::get_task(&conn, &task.id)?
        .ok_or_else(|| AppError::NotFound("That task no longer exists.".into()))
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    Ok(plans::delete_task(&lock(&state.db), &id)?)
}

// --- Data -----------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    data_dir: String,
    db_path: String,
    db_bytes: u64,
    thoughts: i64,
    plans: i64,
    tasks: i64,
}

#[tauri::command]
pub fn get_storage_info(state: State<'_, AppState>) -> AppResult<StorageInfo> {
    let conn = lock(&state.db);
    let count = |table: &str| -> rusqlite::Result<i64> {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
    };
    let db_bytes = ["", "-wal"]
        .iter()
        .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", state.db_path.display())).ok())
        .map(|m| m.len())
        .sum();
    Ok(StorageInfo {
        data_dir: state.data_dir.display().to_string(),
        db_path: state.db_path.display().to_string(),
        db_bytes,
        thoughts: count("thoughts")?,
        plans: count("plans")?,
        tasks: count("tasks")?,
    })
}

#[tauri::command]
pub fn export_data(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return Err(AppError::Invalid("Choose where to save the export.".into()));
    }
    let data = export::export_all(&lock(&state.db), now_ms())?;
    std::fs::write(&path, serde_json::to_vec_pretty(&data)?)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn clear_history(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    export::clear_all(&lock(&state.db))?;
    let _ = app.emit("tf://data-cleared", ());
    Ok(())
}

/// Deletes thoughts, plans, tasks, settings, and the stored API key.
#[tauri::command]
pub fn delete_all_data(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    export::clear_all(&lock(&state.db))?;
    state.keys.delete().map_err(AppError::Keychain)?;
    let _ = app.autolaunch().disable();
    let defaults = state.save_settings(Settings::default())?;
    shortcut::register(&app, &defaults.general.global_shortcut);
    let _ = app.emit("tf://data-cleared", ());
    let _ = app.emit("tf://settings-changed", defaults);
    let _ = app.emit("tf://api-key-changed", state.keys.status());
    Ok(())
}
