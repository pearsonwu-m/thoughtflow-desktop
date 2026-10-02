//! User settings, persisted as JSON in the app data directory.
//!
//! The API key is deliberately not part of settings; it lives in the macOS
//! Keychain (see `secrets.rs`).

use crate::ai::Effort;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_GLOBAL_SHORTCUT: &str = "Alt+Space";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WidgetPosition {
    /// Reopen wherever the user last left it.
    #[default]
    Remember,
    /// Centered horizontally, in the upper third of the active screen.
    Center,
    /// Near the top of the active screen.
    Top,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub global_shortcut: String,
    pub widget_position: WidgetPosition,
    pub theme: Theme,
    pub hide_on_blur: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            global_shortcut: DEFAULT_GLOBAL_SHORTCUT.to_string(),
            widget_position: WidgetPosition::Remember,
            theme: Theme::System,
            hide_on_blur: false,
        }
    }
}

/// How requests reach Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    /// The Messages API, with a key stored in the Keychain.
    #[default]
    Api,
    /// The local Claude Code CLI, with its own sign-in (e.g. a Claude subscription).
    ClaudeCode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeSettings {
    pub connection: Connection,
    /// Path to the `claude` executable; found automatically when unset.
    pub cli_path: Option<String>,
    pub model: String,
    pub effort: Effort,
    /// Only sent to models that accept sampling parameters.
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    /// Offer related past thoughts as context for new ones.
    pub use_memory: bool,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            connection: Connection::Api,
            cli_path: None,
            model: DEFAULT_MODEL.to_string(),
            effort: Effort::Low,
            temperature: None,
            max_tokens: 16_000,
            use_memory: true,
        }
    }
}

/// In-app shortcut bindings, keyed by action. Defaults live in the frontend
/// (`src/lib/shortcuts.ts`); only user overrides are stored here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyboardSettings {
    pub shortcuts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetState {
    /// Last logical position of the widget's top-left corner.
    pub x: Option<f64>,
    pub y: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub claude: ClaudeSettings,
    pub keyboard: KeyboardSettings,
    pub widget: WidgetState,
}

impl Settings {
    /// Loads settings, falling back to defaults for a missing or corrupt file
    /// (a broken settings file should never stop the app from opening).
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    /// Writes atomically: a crash mid-write leaves the previous file intact.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// Clamps values that came from disk or the UI into valid ranges.
    pub fn sanitized(mut self) -> Settings {
        if self.claude.model.trim().is_empty() {
            self.claude.model = DEFAULT_MODEL.to_string();
        }
        self.claude.model = self.claude.model.trim().to_string();
        self.claude.cli_path = self
            .claude
            .cli_path
            .take()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        self.claude.max_tokens = self.claude.max_tokens.clamp(1_024, 64_000);
        self.claude.temperature = self.claude.temperature.map(|t| t.clamp(0.0, 1.0));
        if self.general.global_shortcut.trim().is_empty() {
            self.general.global_shortcut = DEFAULT_GLOBAL_SHORTCUT.to_string();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fill_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"claude": {"effort": "high"}}"#).unwrap();
        assert_eq!(s.claude.effort, Effort::High);
        assert_eq!(s.claude.connection, Connection::Api);
        let cli: Settings =
            serde_json::from_str(r#"{"claude": {"connection": "claudeCode", "cliPath": "  "}}"#)
                .unwrap();
        assert_eq!(cli.claude.connection, Connection::ClaudeCode);
        assert_eq!(cli.sanitized().claude.cli_path, None);
        assert_eq!(s.claude.model, DEFAULT_MODEL);
        assert_eq!(s.general.global_shortcut, "Alt+Space");
        assert!(s.claude.use_memory);
    }

    #[test]
    fn round_trips_through_disk_and_survives_corruption() {
        let dir = std::env::temp_dir().join(format!("tf-settings-{}", crate::util::new_id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let mut s = Settings::default();
        s.general.theme = Theme::Dark;
        s.claude.max_tokens = 999_999;
        s.clone().sanitized().save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.general.theme, Theme::Dark);
        assert_eq!(loaded.claude.max_tokens, 64_000);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::remove_dir_all(dir).ok();
    }
}
