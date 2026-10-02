//! Chooses how requests reach Claude: the Messages API with an API key, or
//! the local Claude Code CLI with its own sign-in. Callers use the same two
//! operations either way.

use super::anthropic::caps::ModelCaps;
use super::anthropic::{self, request, AnthropicClient};
use super::claude_code::ClaudeCodeClient;
use super::{prompts, AiError, ChatOptions, ChatResult, StreamEvent, Turn};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub enum Backend {
    Api(AnthropicClient),
    ClaudeCode(ClaudeCodeClient),
}

impl Backend {
    /// Stored with each reply. API replies keep their full content blocks
    /// (including reasoning, replayed verbatim); Claude Code replies are plain
    /// text, so they replay safely to either backend.
    pub fn provider(&self) -> &'static str {
        match self {
            Backend::Api(_) => "anthropic",
            Backend::ClaudeCode(_) => "claude-code",
        }
    }

    pub async fn chat(
        &self,
        opts: &ChatOptions,
        turns: &[Turn],
        cancel: &CancellationToken,
        emit: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> Result<ChatResult, AiError> {
        match self {
            Backend::Api(client) => {
                let caps = ModelCaps::for_model(&opts.model);
                let built = request::chat_request(opts, &caps, prompts::SYSTEM_PROMPT, turns);
                client.stream_message(&built, cancel, emit).await
            }
            Backend::ClaudeCode(client) => {
                client
                    .stream_chat(opts, prompts::SYSTEM_PROMPT, turns, cancel, emit)
                    .await
            }
        }
    }

    /// One-shot request that returns JSON matching `schema`.
    pub async fn extract(
        &self,
        opts: &ChatOptions,
        system_prompt: &str,
        user_text: &str,
        schema: &Value,
        cancel: &CancellationToken,
    ) -> Result<Value, AiError> {
        match self {
            Backend::Api(client) => {
                let caps = ModelCaps::for_model(&opts.model);
                let built =
                    request::extraction_request(opts, &caps, system_prompt, user_text, schema);
                let text = client.complete_text(&built, cancel).await?;
                anthropic::parse_json_object(&text)
            }
            Backend::ClaudeCode(client) => {
                client
                    .complete_json(opts, system_prompt, user_text, schema, cancel)
                    .await
            }
        }
    }
}
