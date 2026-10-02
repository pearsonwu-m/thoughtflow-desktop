//! The global ⌥Space shortcut.

use super::window;
use crate::state::{lock, AppState, ShortcutStatus};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Renders an accelerator such as `Alt+Space` as `⌥Space`.
pub fn display(accelerator: &str) -> String {
    accelerator
        .split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "alt" | "option" => "⌥".to_string(),
            "shift" => "⇧".to_string(),
            "cmd" | "command" | "super" | "meta" | "cmdorctrl" | "commandorcontrol" => {
                "⌘".to_string()
            }
            "ctrl" | "control" => "⌃".to_string(),
            other => {
                let mut c = other.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect()
}

/// Registers `accelerator` as the widget toggle, replacing any previous one.
/// Failure (e.g. another app owns the combination) is reported, not fatal.
pub fn register(app: &AppHandle, accelerator: &str) -> ShortcutStatus {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let result = shortcuts.on_shortcut(accelerator, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            window::toggle_widget(app);
        }
    });
    let status = match result {
        Ok(()) => ShortcutStatus { accelerator: accelerator.to_string(), registered: true, error: None },
        Err(_) => ShortcutStatus {
            accelerator: accelerator.to_string(),
            registered: false,
            error: Some(format!(
                "Couldn't register {}. Another app may already use it. Choose a different shortcut in Settings → General.",
                display(accelerator)
            )),
        },
    };
    match &status.error {
        None => eprintln!("[thoughtflow] global shortcut {accelerator} registered"),
        Some(_) => eprintln!("[thoughtflow] global shortcut {accelerator} could not be registered"),
    }
    *lock(&app.state::<AppState>().shortcut) = status.clone();
    if !status.registered {
        let _ = app.emit("tf://shortcut-status", status.clone());
    }
    status
}

#[cfg(test)]
mod tests {
    use super::display;

    #[test]
    fn displays_mac_symbols() {
        assert_eq!(display("Alt+Space"), "⌥Space");
        assert_eq!(display("CommandOrControl+Shift+K"), "⌘⇧K");
    }
}
