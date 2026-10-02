//! Provider-agnostic AI types.
//!
//! Everything outside `ai::anthropic` talks in these types, so the Claude
//! integration can be replaced or joined by another provider without touching
//! the conversation, storage, or command layers. Provider-specific wire data is
//! carried opaquely in [`Turn::payload`].

pub mod anthropic;
pub mod prompts;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The thinking mode the user selected for a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Think,
    Plan,
    Task,
    Reflect,
    Prompt,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Think => "think",
            Mode::Plan => "plan",
            Mode::Task => "task",
            Mode::Reflect => "reflect",
            Mode::Prompt => "prompt",
        }
    }

    pub fn parse(s: &str) -> Mode {
        match s {
            "plan" => Mode::Plan,
            "task" => Mode::Task,
            "reflect" => Mode::Reflect,
            "prompt" => Mode::Prompt,
            _ => Mode::Think,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Think => "Think",
            Mode::Plan => "Plan",
            Mode::Task => "Task",
            Mode::Reflect => "Reflect",
            Mode::Prompt => "Prompt",
        }
    }
}

/// How much effort the model should spend. Maps to the API's `effort` control
/// on models that support it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    #[default]
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Assistant,
    /// An app-authored instruction (mode switch, date) appended mid-conversation.
    System,
}

impl TurnRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Assistant => "assistant",
            TurnRole::System => "system",
        }
    }

    pub fn parse(s: &str) -> TurnRole {
        match s {
            "assistant" => TurnRole::Assistant,
            "system" => TurnRole::System,
            _ => TurnRole::User,
        }
    }
}

/// One stored conversation turn, exactly as it was sent to (or received from)
/// the provider. Turns are replayed byte-for-byte so the history stays
/// append-only, which keeps prompt caching and reasoning continuity intact.
#[derive(Debug, Clone)]
pub struct Turn {
    pub role: TurnRole,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub model: String,
    pub effort: Effort,
    pub temperature: Option<f32>,
    pub max_tokens: u32,
}

/// Progress events streamed to the UI while a reply is generated.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StreamEvent {
    /// Text is being sent to the provider (shown as a privacy indicator).
    Sending { model: String },
    /// The provider accepted the request; `model` is the model actually serving it.
    Started { model: String },
    /// The model is reasoning before it writes visible text.
    Thinking,
    /// A transient failure is being retried.
    Retrying {
        attempt: u32,
        delay_ms: u64,
        reason: String,
    },
    /// A chunk of visible reply text.
    Delta { text: String },
}

#[derive(Debug, Clone, Default)]
pub struct ChatResult {
    pub model: String,
    /// Provider wire content to store and replay.
    pub content: Value,
    /// Visible text of the reply.
    pub text: String,
    pub stop_reason: String,
}

impl ChatResult {
    pub fn truncated(&self) -> bool {
        matches!(
            self.stop_reason.as_str(),
            "max_tokens" | "model_context_window_exceeded"
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

/// Failures talking to an AI provider, phrased for humans.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum AiError {
    #[error("Your Anthropic API key was rejected. Check it in Settings → Claude.")]
    InvalidApiKey,
    #[error("This API key doesn't have permission for that request.")]
    PermissionDenied,
    #[error("Your Anthropic account has a billing issue. Check console.anthropic.com.")]
    Billing,
    #[error(
        "The model “{0}” isn't available to your API key. Choose another in Settings → Claude."
    )]
    ModelNotFound(String),
    #[error("{}", rate_limit_message(*.retry_after_secs))]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Claude is temporarily overloaded. Try again in a moment.")]
    Overloaded,
    #[error("Anthropic's API had a problem (HTTP {0}). Try again shortly.")]
    Server(u16),
    #[error("This conversation is too long to send. Start a new thought to continue.")]
    TooLarge,
    #[error("Claude couldn't process that request: {0}")]
    BadRequest(String),
    #[error("Claude couldn't be reached. Check your connection and try again.")]
    Network,
    #[error("Claude took too long to respond. Try again.")]
    Timeout,
    #[error("Claude declined to respond to this. Try rephrasing.")]
    Refusal,
    #[error("Claude's response came back in an unexpected format. Try again.")]
    Malformed(String),
    #[error("Stopped.")]
    Cancelled,
}

fn rate_limit_message(retry_after_secs: Option<u64>) -> String {
    match retry_after_secs {
        Some(s) if s > 0 => {
            format!("Claude is rate-limited right now. Try again in about {s} seconds.")
        }
        _ => "Claude is rate-limited right now. Try again in a moment.".to_string(),
    }
}

impl AiError {
    pub fn kind(&self) -> &'static str {
        match self {
            AiError::InvalidApiKey => "invalidApiKey",
            AiError::PermissionDenied => "permissionDenied",
            AiError::Billing => "billing",
            AiError::ModelNotFound(_) => "modelNotFound",
            AiError::RateLimited { .. } => "rateLimited",
            AiError::Overloaded => "overloaded",
            AiError::Server(_) => "server",
            AiError::TooLarge => "tooLarge",
            AiError::BadRequest(_) => "badRequest",
            AiError::Network => "network",
            AiError::Timeout => "timeout",
            AiError::Refusal => "refusal",
            AiError::Malformed(_) => "malformed",
            AiError::Cancelled => "cancelled",
        }
    }

    /// Whether retrying the identical request could plausibly succeed.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            AiError::RateLimited { .. }
                | AiError::Overloaded
                | AiError::Server(_)
                | AiError::Network
                | AiError::Timeout
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_events_serialize_for_the_frontend() {
        let ev = StreamEvent::Retrying {
            attempt: 1,
            delay_ms: 500,
            reason: "overloaded".into(),
        };
        assert_eq!(
            serde_json::to_value(ev).unwrap(),
            serde_json::json!({"type": "retrying", "attempt": 1, "delayMs": 500, "reason": "overloaded"})
        );
        assert_eq!(
            serde_json::to_value(StreamEvent::Thinking).unwrap(),
            serde_json::json!({"type": "thinking"})
        );
    }

    #[test]
    fn errors_read_like_sentences() {
        assert_eq!(
            AiError::Network.to_string(),
            "Claude couldn't be reached. Check your connection and try again."
        );
        assert!(AiError::RateLimited {
            retry_after_secs: Some(12)
        }
        .to_string()
        .contains("12 seconds"));
        assert!(AiError::Overloaded.retryable());
        assert!(!AiError::InvalidApiKey.retryable());
    }
}
