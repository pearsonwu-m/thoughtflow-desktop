//! The error type returned by every Tauri command.
//!
//! Errors cross into the UI as `{ kind, message, retryable }`, where `message`
//! is a short sentence safe to show as-is. Raw thought content is never
//! included in error messages or logs.

use crate::ai::AiError;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Add your Anthropic API key in Settings → Claude to think with Claude.")]
    MissingApiKey,
    #[error(transparent)]
    Ai(#[from] AiError),
    #[error("Thoughtflow couldn't read or write its local database. ({0})")]
    Database(#[from] rusqlite::Error),
    #[error("{0}")]
    Keychain(String),
    #[error("Couldn't access a file: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::MissingApiKey => "missingApiKey",
            AppError::Ai(e) => e.kind(),
            AppError::Database(_) => "database",
            AppError::Keychain(_) => "keychain",
            AppError::Io(_) => "io",
            AppError::NotFound(_) => "notFound",
            AppError::Invalid(_) => "invalid",
            AppError::Internal(_) => "internal",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, AppError::Ai(e) if e.retryable())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(format!("Couldn't encode data: {e}"))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("retryable", &self.retryable())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_for_the_ui() {
        let value = serde_json::to_value(AppError::Ai(AiError::Network)).unwrap();
        assert_eq!(value["kind"], "network");
        assert_eq!(value["retryable"], true);
        assert_eq!(
            value["message"],
            "Claude couldn't be reached. Check your connection and try again."
        );
        assert_eq!(
            serde_json::to_value(AppError::MissingApiKey).unwrap()["kind"],
            "missingApiKey"
        );
    }
}
