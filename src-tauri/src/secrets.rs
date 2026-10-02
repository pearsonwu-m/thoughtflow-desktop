//! Anthropic API key storage.
//!
//! The key is stored in the macOS Keychain (via the `keyring` crate) and is
//! only ever read by the Rust process. For development, `ANTHROPIC_API_KEY`
//! from the environment or a local `.env` file is used when no Keychain item
//! exists.

use serde::Serialize;
use std::sync::Mutex;

const SERVICE: &str = "com.thoughtflow.desktop";
const ACCOUNT: &str = "anthropic-api-key";
const ENV_VAR: &str = "ANTHROPIC_API_KEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeySource {
    Keychain,
    Environment,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyStatus {
    pub configured: bool,
    pub source: KeySource,
    /// e.g. `sk-ant-…a1b2`; never the full key.
    pub hint: Option<String>,
}

/// Caches the key in memory so the Keychain is read once per session.
#[derive(Default)]
pub struct KeyStore {
    cached: Mutex<Option<Option<(String, KeySource)>>>,
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("Couldn't open the Keychain: {e}"))
}

pub fn mask(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("sk-ant-…{tail}")
}

pub fn validate_format(key: &str) -> Result<(), String> {
    let key = key.trim();
    if !key.starts_with("sk-ant-") || key.len() < 24 || key.chars().any(char::is_whitespace) {
        return Err(
            "That doesn't look like an Anthropic API key. Keys start with “sk-ant-”.".into(),
        );
    }
    Ok(())
}

impl KeyStore {
    pub fn get(&self) -> Option<(String, KeySource)> {
        let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = cached.as_ref() {
            return value.clone();
        }
        let value = read_keychain()
            .map(|k| (k, KeySource::Keychain))
            .or_else(|| {
                std::env::var(ENV_VAR)
                    .ok()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .map(|k| (k, KeySource::Environment))
            });
        *cached = Some(value.clone());
        value
    }

    pub fn status(&self) -> ApiKeyStatus {
        match self.get() {
            Some((key, source)) => ApiKeyStatus {
                configured: true,
                source,
                hint: Some(mask(&key)),
            },
            None => ApiKeyStatus {
                configured: false,
                source: KeySource::None,
                hint: None,
            },
        }
    }

    pub fn set(&self, key: &str) -> Result<(), String> {
        let key = key.trim();
        validate_format(key)?;
        entry()?
            .set_password(key)
            .map_err(|e| format!("Couldn't save the key to the Keychain: {e}"))?;
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Some((key.to_string(), KeySource::Keychain)));
        Ok(())
    }

    pub fn delete(&self) -> Result<(), String> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("Couldn't remove the key from the Keychain: {e}")),
        }
        // Re-resolve so an environment key (dev only) is picked up again.
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

fn read_keychain() -> Option<String> {
    entry()
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_all_but_the_tail() {
        assert_eq!(mask("sk-ant-api03-abcdefghijklmnop-wxyz"), "sk-ant-…wxyz");
    }

    #[test]
    fn rejects_malformed_keys() {
        assert!(validate_format("sk-ant-api03-0123456789abcdefghij").is_ok());
        assert!(validate_format("sk-proj-0123456789abcdefghij").is_err());
        assert!(validate_format("sk-ant-short").is_err());
        assert!(validate_format("sk-ant-api03-0123456789 abcdefghij").is_err());
    }
}
