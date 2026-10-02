//! Interprets `claude -p --output-format stream-json` output, one JSON line at
//! a time. `stream_event` lines wrap raw Messages API stream events, so they
//! reuse the Anthropic accumulator; the final `result` line is authoritative.

use crate::ai::anthropic::accumulate::{Applied, MessageAccumulator};
use crate::ai::{AiError, ChatResult};
use crate::util::truncate_chars;
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
pub enum CliEvent {
    Nothing,
    Started(String),
    Thinking,
    Text(String),
}

#[derive(Debug, Clone)]
pub struct CliResult {
    pub is_error: bool,
    pub text: String,
    pub stop_reason: Option<String>,
    pub structured: Option<Value>,
}

#[derive(Debug, Default)]
pub struct CliStream {
    acc: MessageAccumulator,
    model: String,
    error_code: Option<String>,
    result: Option<CliResult>,
}

impl CliStream {
    pub fn apply_line(&mut self, line: &str) -> CliEvent {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return CliEvent::Nothing; // Non-JSON noise is ignored.
        };
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                self.model = v["model"].as_str().unwrap_or_default().to_string();
                CliEvent::Started(self.model.clone())
            }
            "stream_event" => {
                let event = &v["event"];
                if event["type"] == "message_start" {
                    self.acc = MessageAccumulator::default();
                }
                match self.acc.apply(event) {
                    Ok(Applied::Text(text)) => CliEvent::Text(text),
                    Ok(Applied::ThinkingStarted) => CliEvent::Thinking,
                    Ok(Applied::Started(model)) if !model.is_empty() => {
                        self.model = model;
                        CliEvent::Nothing
                    }
                    // Stream errors are reported again in the final result.
                    _ => CliEvent::Nothing,
                }
            }
            "assistant" => {
                if let Some(code) = v["error"].as_str() {
                    self.error_code = Some(code.to_string());
                }
                CliEvent::Nothing
            }
            "result" => {
                self.result = Some(parse_result(&v));
                CliEvent::Nothing
            }
            _ => CliEvent::Nothing,
        }
    }

    pub fn has_result(&self) -> bool {
        self.result.is_some()
    }

    /// The finished reply, stored as plain text so it can be replayed to any
    /// provider later.
    pub fn finish(self, requested_model: &str) -> Result<ChatResult, AiError> {
        let result = self
            .result
            .ok_or_else(|| AiError::ClaudeCode("it stopped before replying.".into()))?;
        if result.is_error {
            return Err(map_error(
                self.error_code.as_deref(),
                &result.text,
                requested_model,
            ));
        }
        if result.stop_reason.as_deref() == Some("refusal") {
            return Err(AiError::Refusal);
        }
        let text = result.text.trim().to_string();
        if text.is_empty() {
            return Err(AiError::Malformed("empty reply from Claude Code".into()));
        }
        let model = if self.model.is_empty() {
            requested_model.to_string()
        } else {
            self.model
        };
        Ok(ChatResult {
            model,
            content: json!([{"type": "text", "text": text}]),
            text,
            stop_reason: result.stop_reason.unwrap_or_else(|| "end_turn".into()),
        })
    }
}

pub fn parse_result(v: &Value) -> CliResult {
    CliResult {
        is_error: v["is_error"].as_bool().unwrap_or(false)
            || v["subtype"]
                .as_str()
                .is_some_and(|s| s.starts_with("error")),
        text: v["result"].as_str().unwrap_or_default().to_string(),
        stop_reason: v["stop_reason"].as_str().map(str::to_string),
        structured: v.get("structured_output").filter(|s| !s.is_null()).cloned(),
    }
}

fn looks_signed_out(message: &str) -> bool {
    let m = message.to_lowercase();
    [
        "/login",
        "not logged in",
        "log in",
        "please run claude",
        "invalid api key",
        "oauth token",
        "authentication",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

/// Maps Claude Code's error codes (and messages) to app errors.
pub fn map_error(code: Option<&str>, message: &str, model: &str) -> AiError {
    match code {
        Some("authentication_failed") => AiError::ClaudeCodeSignedOut,
        Some("billing_error") => AiError::Billing,
        Some("model_not_found") => AiError::ModelNotFound(model.to_string()),
        Some("server_error") => AiError::Overloaded,
        Some("invalid_request") => AiError::BadRequest(truncate_chars(message, 220)),
        _ if looks_signed_out(message) => AiError::ClaudeCodeSignedOut,
        // Usage limits and anything else: Claude Code's own wording is the most
        // useful (it includes when a limit resets).
        _ if !message.trim().is_empty() => AiError::ClaudeCode(truncate_chars(message, 220)),
        _ => AiError::ClaudeCode("it reported an error without details.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from real `claude -p --output-format stream-json --verbose
    // --include-partial-messages` output (Claude Code 2.1.287).
    const SUCCESS: &[&str] = &[
        r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","tools":[],"mcp_servers":[]}"#,
        r#"{"type":"system","subtype":"status"}"#,
        r#"{"type":"stream_event","event":{"type":"message_start","message":{"model":"claude-opus-5-5","content":[],"stop_reason":null}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello from thoughtfl"}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ow"}}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello from thoughtflow"}]}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
        r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#,
        r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
        r#"{"type":"rate_limit_event","rate_limit_info":{}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"hello from thoughtflow","stop_reason":"end_turn"}"#,
    ];

    #[test]
    fn streams_text_and_finishes_as_plain_text() {
        let mut stream = CliStream::default();
        let events: Vec<CliEvent> = SUCCESS.iter().map(|l| stream.apply_line(l)).collect();
        assert_eq!(events[0], CliEvent::Started("claude-opus-5-5".into()));
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                CliEvent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello from thoughtflow");
        let result = stream.finish("claude-opus-5-5").unwrap();
        assert_eq!(result.text, "hello from thoughtflow");
        assert_eq!(
            result.content,
            json!([{"type": "text", "text": "hello from thoughtflow"}])
        );
    }

    #[test]
    fn maps_cli_errors() {
        let mut stream = CliStream::default();
        stream.apply_line(
            r#"{"type":"assistant","error":"model_not_found","message":{"content":[]}}"#,
        );
        stream.apply_line(r#"{"type":"result","subtype":"success","is_error":true,"result":"There's an issue with the selected model (x)."}"#);
        assert_eq!(
            stream.finish("x").unwrap_err(),
            AiError::ModelNotFound("x".into())
        );

        assert_eq!(
            map_error(Some("authentication_failed"), "", "m"),
            AiError::ClaudeCodeSignedOut
        );
        assert_eq!(
            map_error(None, "Not logged in · Please run /login", "m"),
            AiError::ClaudeCodeSignedOut
        );
        assert_eq!(
            map_error(
                Some("rate_limit"),
                "You've hit your limit · resets 3pm",
                "m"
            ),
            AiError::ClaudeCode("You've hit your limit · resets 3pm".into())
        );
    }

    #[test]
    fn missing_result_and_refusals_are_errors() {
        let mut stream = CliStream::default();
        stream.apply_line("not json at all");
        assert!(matches!(stream.finish("m"), Err(AiError::ClaudeCode(_))));

        let mut stream = CliStream::default();
        stream.apply_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"","stop_reason":"refusal"}"#);
        assert_eq!(stream.finish("m").unwrap_err(), AiError::Refusal);
    }

    #[test]
    fn reads_structured_output() {
        let r = parse_result(
            &json!({"type": "result", "subtype": "success", "is_error": false, "result": "{}", "structured_output": {"tasks": []}}),
        );
        assert_eq!(r.structured, Some(json!({"tasks": []})));
    }
}
