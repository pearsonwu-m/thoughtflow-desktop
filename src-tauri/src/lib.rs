//! Thoughtflow: a persistent, minimalist desktop thinking companion.
//!
//! Layout:
//! - `ai`            provider-agnostic AI types; `ai::anthropic` is the Claude client
//! - `conversation`  request building from memory, streaming, persistence
//! - `db`            local SQLite storage and full-text memory retrieval
//! - `desktop`       widget window, tray, app menu, global shortcut
//! - `settings`      persisted preferences; `secrets` holds the API key (Keychain)
//! - `commands`      the Tauri command surface exposed to the webview

mod ai;
mod commands;
mod conversation;
mod db;
mod desktop;
mod error;
mod secrets;
mod settings;
mod state;
mod util;

use desktop::{shortcut, tray, window};
use secrets::KeyStore;
use settings::Settings;
use state::{AppState, ShortcutStatus};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tauri::{App, Manager, RunEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Passed by the login item so a login launch stays quietly in the menu bar.
const BACKGROUND_ARG: &str = "--background";

pub fn run() {
    // Development convenience: read ANTHROPIC_API_KEY from a local .env file.
    // `tauri dev` runs from src-tauri/, so also look one level up.
    #[cfg(debug_assertions)]
    {
        if dotenvy::dotenv().is_err() {
            let _ = dotenvy::from_filename("../.env");
        }
    }

    let app = tauri::Builder::default()
        // Must be first: launching again forwards to the running instance, so
        // `Thoughtflow --toggle` (etc.) can be bound in launchers and scripts.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            handle_cli(app, &args);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![BACKGROUND_ARG]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            setup(app)?;
            Ok(())
        })
        .on_window_event(window::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::update_settings,
            commands::get_shortcut_status,
            commands::get_api_key_status,
            commands::set_api_key,
            commands::delete_api_key,
            commands::test_connection,
            commands::claude_code_status,
            commands::show_widget,
            commands::hide_widget,
            commands::resize_widget,
            commands::open_settings,
            commands::take_settings_section,
            commands::quit_app,
            commands::list_thoughts,
            commands::get_thought,
            commands::capture_thought,
            commands::rename_thought,
            commands::set_thought_tags,
            commands::set_thought_memory,
            commands::delete_thought,
            commands::find_related,
            commands::send_message,
            commands::regenerate_response,
            commands::cancel_message,
            commands::extract_plan,
            commands::extract_tasks,
            commands::update_prompt,
            commands::list_plans,
            commands::update_plan,
            commands::delete_plan,
            commands::list_tasks,
            commands::create_task,
            commands::update_task,
            commands::delete_task,
            commands::get_storage_info,
            commands::export_data,
            commands::clear_history,
            commands::delete_all_data,
        ])
        .build(tauri::generate_context!())
        .expect("Thoughtflow failed to start");

    app.run(|_app, event| {
        // Closing the last window must not quit a menu-bar app; only an
        // explicit Quit (which passes an exit code) does.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}

/// Actions for a second launch: `--toggle`, `--hide`, `--new`, `--history`,
/// `--settings`. With no flag, the widget is shown.
fn handle_cli(app: &tauri::AppHandle, args: &[String]) {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--toggle") {
        window::toggle_widget(app);
    } else if has("--hide") {
        window::hide_widget(app);
    } else if has("--settings") {
        window::open_settings(app, None);
    } else if has("--history") {
        window::show_widget(app, Some("history"));
    } else if has("--new") {
        window::show_widget(app, Some("new"));
    } else {
        window::show_widget(app, None);
    }
}

/// First-run default: without an API key, use Claude Code when it's installed,
/// so Thoughtflow works out of the box with a Claude subscription. The choice
/// is saved either way and never revisited automatically.
fn choose_default_connection(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    if state.keys.get().is_none()
        && ai::claude_code::find_cli(settings.claude.cli_path.as_deref()).is_some()
    {
        settings.claude.connection = settings::Connection::ClaudeCode;
        eprintln!("[thoughtflow] no API key found; connecting through Claude Code");
    }
    if let Ok(saved) = state.save_settings(settings) {
        let _ = app.emit("tf://settings-changed", saved);
    }
}

/// Opens the database, setting a damaged file aside instead of refusing to start.
fn open_database(
    path: &Path,
) -> Result<(rusqlite::Connection, Option<String>), Box<dyn std::error::Error>> {
    match db::open(path) {
        Ok(conn) => Ok((conn, None)),
        Err(_) => {
            let backup = path.with_extension(format!("db.damaged-{}", util::now_ms()));
            std::fs::rename(path, &backup)?;
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
            }
            let conn = db::open(path)?;
            Ok((
                conn,
                Some(format!(
                    "Thoughtflow's database couldn't be opened, so a fresh one was created. The old file was kept at {}.",
                    backup.display()
                )),
            ))
        }
    }
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // Menu-bar app: no Dock icon, and the widget can float over full-screen apps.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    // Lifecycle logs never include thought content.
    eprintln!("[thoughtflow] data directory: {}", data_dir.display());
    let db_path = data_dir.join("thoughtflow.db");
    let settings_path = data_dir.join("settings.json");
    let settings = Settings::load(&settings_path);
    // Settings written before the Claude Code option existed have no
    // connection yet; one is chosen once, below.
    let connection_chosen = std::fs::read_to_string(&settings_path)
        .map(|raw| raw.contains("\"connection\""))
        .unwrap_or(false);
    let (conn, startup_warning) = open_database(&db_path)?;

    app.manage(AppState {
        db: Mutex::new(conn),
        db_path,
        data_dir,
        settings: Mutex::new(settings.clone()),
        settings_path,
        keys: KeyStore::default(),
        http: ai::anthropic::http_client(),
        inflight: Mutex::new(HashMap::new()),
        shortcut: Mutex::new(ShortcutStatus::default()),
        pending_settings_section: Mutex::new(None),
        startup_warning: Mutex::new(startup_warning),
    });

    let handle = app.handle().clone();
    if let Some(widget) = window::widget(&handle) {
        window::configure_widget(&widget);
    }
    app.set_menu(tray::app_menu(&handle)?)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref().starts_with("app:") {
            tray::handle_menu_event(app, &event);
        }
    });
    tray::create_tray(&handle)?;
    shortcut::register(&handle, &settings.general.global_shortcut);
    if !connection_chosen {
        let handle = handle.clone();
        // Off the main thread: reads the Keychain and may ask the login shell for PATH.
        std::thread::spawn(move || choose_default_connection(&handle));
    }

    // Keep the login item in sync with the saved preference.
    let autolaunch = handle.autolaunch();
    if autolaunch.is_enabled().unwrap_or(false) != settings.general.launch_at_login {
        let _ = if settings.general.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
    }

    let background = std::env::args().any(|a| a == BACKGROUND_ARG);
    if !background {
        window::show_widget(&handle, None);
    }
    Ok(())
}
