//! The floating widget and the settings window.

use crate::settings::WidgetPosition;
use crate::state::{lock, AppState};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

pub const WIDGET: &str = "widget";
pub const SETTINGS: &str = "settings";
/// Logical width of the widget window (the card plus its shadow margin).
pub const WIDGET_WIDTH: f64 = 600.0;
const MIN_HEIGHT: f64 = 140.0;
/// Matches the CSS exit animation in `src/styles/widget.css`.
const HIDE_ANIMATION: Duration = Duration::from_millis(110);

/// Incremented on every show/hide so a delayed hide can tell whether the
/// widget was summoned again in the meantime.
static VISIBILITY_EPOCH: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShownPayload {
    view: Option<String>,
}

pub fn widget(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WIDGET)
}

/// Lets the widget appear over whatever is on screen, including full-screen
/// apps: each time it is shown it moves to the active Space. It is also kept
/// out of the ⌘` window cycle. (Tauri's "visible on all workspaces" does not
/// cover full-screen Spaces.)
#[cfg(target_os = "macos")]
pub fn configure_widget(window: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    let Ok(ptr) = window.ns_window() else { return };
    // SAFETY: Tauri returns the live NSWindow backing this webview window, and
    // this runs during setup on the main thread.
    let ns_window = unsafe { &*(ptr as *const NSWindow) };
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}

#[cfg(not(target_os = "macos"))]
pub fn configure_widget(_window: &WebviewWindow) {}

/// Unhides the app, orders the widget in, and activates the app, in that
/// order, on the main thread. (Issued separately, the window can be ordered
/// in before the app is unhidden and stay invisible.)
#[cfg(target_os = "macos")]
fn present(app: &AppHandle, window: &WebviewWindow) {
    let window = window.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSWindow};
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let ns_app = NSApplication::sharedApplication(mtm);
        ns_app.unhide(None);
        if let Ok(ptr) = window.ns_window() {
            // SAFETY: the live NSWindow for this webview window, used on the main thread.
            let ns_window = unsafe { &*(ptr as *const NSWindow) };
            ns_window.makeKeyAndOrderFront(None);
        }
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);
    });
}

#[cfg(not(target_os = "macos"))]
fn present(_app: &AppHandle, window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

fn contains(m: &Monitor, x: f64, y: f64) -> bool {
    let p = m.position();
    let s = m.size();
    x >= p.x as f64
        && x < p.x as f64 + s.width as f64
        && y >= p.y as f64
        && y < p.y as f64 + s.height as f64
}

/// The screen the user is working on: the one under the mouse pointer.
fn active_monitor(app: &AppHandle) -> Option<Monitor> {
    let monitors = app.available_monitors().ok()?;
    if let Ok(cursor) = app.cursor_position() {
        if let Some(m) = monitors.iter().find(|m| contains(m, cursor.x, cursor.y)) {
            return Some(m.clone());
        }
    }
    app.primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next())
}

fn place_widget(app: &AppHandle, window: &WebviewWindow) {
    let settings = app.state::<AppState>().settings();
    let mode = settings.general.widget_position;
    if mode == WidgetPosition::Remember {
        if let (Some(x), Some(y)) = (settings.widget.x, settings.widget.y) {
            let visible = app
                .available_monitors()
                .map(|ms| ms.iter().any(|m| contains(m, x + 40.0, y + 20.0)))
                .unwrap_or(false);
            if visible {
                let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
                return;
            }
        }
    }
    let Some(monitor) = active_monitor(app) else {
        let _ = window.center();
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width = WIDGET_WIDTH * scale;
    let x = area.position.x as f64 + (area.size.width as f64 - width) / 2.0;
    let ratio = if mode == WidgetPosition::Top {
        0.06
    } else {
        0.2
    };
    let y = area.position.y as f64 + area.size.height as f64 * ratio;
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// Shows and focuses the widget, optionally opening a specific view
/// (`compose`, `new`, `history`).
pub fn show_widget(app: &AppHandle, view: Option<&str>) {
    let Some(window) = widget(app) else { return };
    VISIBILITY_EPOCH.fetch_add(1, Ordering::SeqCst);
    if !window.is_visible().unwrap_or(false) {
        place_widget(app, &window);
    }
    present(app, &window);
    let _ = app.emit_to(
        WIDGET,
        "tf://shown",
        ShownPayload {
            view: view.map(str::to_string),
        },
    );
}

/// Plays the exit animation, then hides the widget and returns focus to the
/// previously active app.
pub fn hide_widget(app: &AppHandle) {
    let Some(window) = widget(app) else { return };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    remember_position(app);
    let epoch = VISIBILITY_EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit_to(WIDGET, "tf://hiding", ());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDE_ANIMATION);
        if VISIBILITY_EPOCH.load(Ordering::SeqCst) != epoch {
            return; // Summoned again during the animation.
        }
        if let Some(window) = widget(&app) {
            let _ = window.hide();
        }
        #[cfg(target_os = "macos")]
        {
            let settings_open = app
                .get_webview_window(SETTINGS)
                .is_some_and(|w| w.is_visible().unwrap_or(false));
            if !settings_open {
                let _ = app.hide();
            }
        }
    });
}

/// ⌥Space behavior: open when hidden, focus when open but in the background,
/// close when open and focused.
pub fn toggle_widget(app: &AppHandle) {
    match widget(app) {
        Some(w) if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) => {
            hide_widget(app)
        }
        _ => show_widget(app, None),
    }
}

pub fn remember_position(app: &AppHandle) {
    let Some(window) = widget(app) else { return };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    if settings.widget.x == Some(pos.x as f64) && settings.widget.y == Some(pos.y as f64) {
        return;
    }
    settings.widget.x = Some(pos.x as f64);
    settings.widget.y = Some(pos.y as f64);
    let _ = state.save_settings(settings);
}

/// Grows or shrinks the widget vertically to fit its content, keeping it on
/// screen.
pub fn resize_widget(window: &WebviewWindow, height: f64) -> tauri::Result<()> {
    let scale = window.scale_factor()?;
    let mut height = height.max(MIN_HEIGHT);
    if let Some(monitor) = window.current_monitor()? {
        let area = monitor.work_area();
        let max_height = area.size.height as f64 / scale - 32.0;
        height = height.min(max_height);
        let pos = window.outer_position()?;
        let bottom = pos.y as f64 + height * scale;
        let area_bottom = area.position.y as f64 + area.size.height as f64;
        if bottom > area_bottom {
            let y = (area_bottom - height * scale).max(area.position.y as f64);
            window.set_position(PhysicalPosition::new(pos.x, y.round() as i32))?;
        }
    }
    window.set_size(LogicalSize::new(WIDGET_WIDTH, height.round()))
}

pub fn open_settings(app: &AppHandle, section: Option<&str>) {
    let state = app.state::<AppState>();
    *lock(&state.pending_settings_section) = section.map(str::to_string);
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(section) = section {
            let _ = app.emit_to(SETTINGS, "tf://settings-section", section.to_string());
        }
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html".into()))
        .title("Thoughtflow Settings")
        .inner_size(780.0, 560.0)
        .min_inner_size(680.0, 480.0)
        .resizable(true)
        .maximizable(false)
        .center()
        .focused(true)
        .build();
    if let Ok(window) = built {
        let _ = window.set_focus();
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != WIDGET {
        return;
    }
    let app = window.app_handle();
    match event {
        // ⌘W or a close request hides the widget; it is never destroyed.
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_widget(app);
        }
        WindowEvent::Focused(false) if app.state::<AppState>().settings().general.hide_on_blur => {
            hide_widget(app)
        }
        _ => {}
    }
}
