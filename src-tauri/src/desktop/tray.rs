//! Menu-bar icon and the macOS application menu.

use super::window;
use tauri::image::Image;
use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};

const TRAY_ICON: &[u8] = include_bytes!("../../icons/tray-icon.png");

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "tray:open", "Open Thoughtflow", true, None::<&str>)?,
            &MenuItem::with_id(app, "tray:new", "New Thought", true, None::<&str>)?,
            &MenuItem::with_id(app, "tray:search", "Search Thoughts", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "tray:settings", "Settings…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "tray:quit", "Quit Thoughtflow", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("thoughtflow")
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip("Thoughtflow")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if event.id().as_ref().starts_with("tray:") {
                handle_menu_event(app, &event);
            }
        })
        .build(app)?;
    Ok(())
}

/// The app menu stays mostly invisible (Thoughtflow has no Dock icon), but
/// it provides the standard editing key equivalents (⌘C, ⌘V, ⌘Z…) inside
/// text fields. It intentionally omits "Hide" so ⌘H reaches the widget.
pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let about = AboutMetadata {
        name: Some("Thoughtflow".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        comments: Some("A minimalist desktop thinking companion.".into()),
        ..Default::default()
    };
    let app_submenu = Submenu::with_items(
        app,
        "Thoughtflow",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Thoughtflow"), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "app:settings", "Settings…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "app:quit",
                "Quit Thoughtflow",
                true,
                Some("CmdOrCtrl+Q"),
            )?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::close_window(app, None)?],
    )?;
    Menu::with_items(app, &[&app_submenu, &edit, &window_menu])
}

pub fn quit(app: &AppHandle) {
    window::remember_position(app);
    app.exit(0);
}

pub fn handle_menu_event(app: &AppHandle, event: &MenuEvent) {
    match event.id().as_ref() {
        "tray:open" => window::show_widget(app, Some("compose")),
        "tray:new" => window::show_widget(app, Some("new")),
        "tray:search" => window::show_widget(app, Some("history")),
        "tray:settings" | "app:settings" => window::open_settings(app, None),
        "tray:quit" | "app:quit" => quit(app),
        _ => {}
    }
}
