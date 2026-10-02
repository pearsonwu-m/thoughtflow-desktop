//! Shared application state managed by Tauri.

use crate::secrets::KeyStore;
use crate::settings::Settings;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub accelerator: String,
    pub registered: bool,
    pub error: Option<String>,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub keys: KeyStore,
    pub http: reqwest::Client,
    /// In-flight Claude requests by request id, so the UI can stop them.
    pub inflight: Mutex<HashMap<String, CancellationToken>>,
    pub shortcut: Mutex<ShortcutStatus>,
    /// Settings section to open when the settings window first loads.
    pub pending_settings_section: Mutex<Option<String>>,
    /// A problem found at startup (e.g. a damaged database was set aside).
    pub startup_warning: Mutex<Option<String>>,
}

/// Locks a mutex, recovering the data if a previous holder panicked. A panic
/// elsewhere should never brick storage or settings for the rest of the session.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn save_settings(&self, settings: Settings) -> std::io::Result<Settings> {
        let settings = settings.sanitized();
        settings.save(&self.settings_path)?;
        *lock(&self.settings) = settings.clone();
        Ok(settings)
    }

    pub fn begin_request(&self, id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        lock(&self.inflight).insert(id.to_string(), token.clone());
        token
    }

    pub fn end_request(&self, id: &str) {
        lock(&self.inflight).remove(id);
    }

    pub fn cancel_request(&self, id: &str) -> bool {
        match lock(&self.inflight).remove(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}
