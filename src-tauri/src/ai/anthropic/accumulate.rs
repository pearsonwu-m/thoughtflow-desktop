//! Rebuilds a complete assistant message from Messages API stream events.
//!
//! Every content block is kept, including `thinking` blocks and their
//! signatures, because the next request must replay the assistant turn
//! unchanged for the model to keep its reasoning.

use super::errors::error_from_body;
use crate::ai::{AiError, ChatResult};
use serde_json::{json, Value};
use std::collections::HashMap;

/// What a single stream event meant for the UI.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    Nothing,
    Started(String),
    ThinkingStarted,
    Text(String),
    Stopped,
}

#[derive(Debug, Default)]
pub struct MessageAccumulator {
    model: String,
    blocks: Vec<Value>,
    partial_json: HashMap<usize, String>,
    stop_reason: Option<String>,
    finished: bool,
}

impl MessageAccumulator {
    pub fn apply(&mut self, event: &Value) -> Result<Applied, AiError> {
        match event["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let message = &event["message"];
                self.model = message["model"].as_str().unwrap_or_default().to_string();
                if let Some(content) = message["content"].as_array() {
                    self.blocks.extend(content.iter().cloned());
                }
                Ok(Applied::Started(self.model.clone()))
            }
            "content_block_start" => {
                let index = event["index"].as_u64().unwrap_or(self.blocks.len() as u64) as usize;
                let block = event["content_block"].clone();
                let kind = block["type"].as_str().unwrap_or_default().to_string();
                let initial_text = block["text"].as_str().unwrap_or_default().to_string();
                self.slot(index);
                self.blocks[index] = block;
                Ok(match kind.as_str() {
                    "thinking" | "redacted_thinking" => Applied::ThinkingStarted,
                    "text" if !initial_text.is_empty() => Applied::Text(initial_text),
                    _ => Applied::Nothing,
                })
            }
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                self.slot(index);
                let delta = &event["delta"];
                let block = &mut self.blocks[index];
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        append_str(block, "text", text);
                        Ok(Applied::Text(text.to_string()))
                    }
                    "thinking_delta" => {
                        append_str(
                            block,
                            "thinking",
                            delta["thinking"].as_str().unwrap_or_default(),
                        );
                        Ok(Applied::Nothing)
                    }
                    "signature_delta" => {
                        block["signature"] = delta["signature"].clone();
                        Ok(Applied::Nothing)
                    }
                    "input_json_delta" => {
                        self.partial_json
                            .entry(index)
                            .or_default()
                            .push_str(delta["partial_json"].as_str().unwrap_or_default());
                        Ok(Applied::Nothing)
                    }
                    "citations_delta" => {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        if let Some(list) = block["citations"].as_array_mut() {
                            list.push(delta["citation"].clone());
                        }
                        Ok(Applied::Nothing)
                    }
                    _ => Ok(Applied::Nothing),
                }
            }
            "content_block_stop" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                if let Some(raw) = self.partial_json.remove(&index) {
                    if let (Some(block), Ok(input)) =
                        (self.blocks.get_mut(index), serde_json::from_str(&raw))
                    {
                        block["input"] = input;
                    }
                }
                Ok(Applied::Nothing)
            }
            "message_delta" => {
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_string());
                }
                Ok(Applied::Nothing)
            }
            "message_stop" => {
                self.finished = true;
                Ok(Applied::Stopped)
            }
            "error" => Err(error_from_body(event, 0, &self.model)),
            // `ping` and event types added in the future are ignored.
            _ => Ok(Applied::Nothing),
        }
    }

    fn slot(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(Value::Null);
        }
    }

    pub fn finish(self) -> Result<ChatResult, AiError> {
        if !self.finished {
            return Err(AiError::Network);
        }
        let stop_reason = self.stop_reason.unwrap_or_else(|| "end_turn".to_string());
        if stop_reason == "refusal" {
            return Err(AiError::Refusal);
        }
        let content = sanitize_for_replay(self.blocks);
        let text = visible_text(&content);
        Ok(ChatResult {
            model: self.model,
            content: Value::Array(content),
            text,
            stop_reason,
        })
    }
}

fn append_str(block: &mut Value, key: &str, text: &str) {
    let current = block[key].as_str().unwrap_or_default();
    block[key] = Value::String(format!("{current}{text}"));
}

/// Prepares assistant content for storage and replay.
///
/// After a server-side refusal fallback, model-internal blocks produced before
/// the final `fallback` marker must not be echoed back. The marker itself is an
/// audit record the API ignores, so it is dropped too; that keeps stored turns
/// valid even if the user later switches to a model without fallbacks.
pub fn sanitize_for_replay(blocks: Vec<Value>) -> Vec<Value> {
    let last_fallback = blocks.iter().rposition(|b| b["type"] == "fallback");
    blocks
        .into_iter()
        .enumerate()
        .filter(|(i, block)| {
            let kind = block["type"].as_str().unwrap_or_default();
            if block.is_null() || kind == "fallback" {
                return false;
            }
            let before_fallback = last_fallback.is_some_and(|lf| *i < lf);
            !(before_fallback
                && matches!(
                    kind,
                    "thinking" | "redacted_thinking" | "tool_use" | "server_tool_use"
                ))
        })
        .map(|(_, block)| block)
        .collect()
}

/// The user-visible text of a message: its text blocks, in order.
pub fn visible_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(acc: &mut MessageAccumulator, events: &[Value]) -> Vec<Applied> {
        events.iter().map(|e| acc.apply(e).unwrap()).collect()
    }

    #[test]
    fn rebuilds_thinking_and_text_blocks() {
        let mut acc = MessageAccumulator::default();
        let applied = feed(
            &mut acc,
            &[
                json!({"type": "message_start", "message": {"model": "claude-opus-5-5", "content": []}}),
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig=="}}),
                json!({"type": "content_block_stop", "index": 0}),
                json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
                json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hello"}}),
                json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": " there"}}),
                json!({"type": "content_block_stop", "index": 1}),
                json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}}),
                json!({"type": "message_stop"}),
            ],
        );
        assert_eq!(applied[0], Applied::Started("claude-opus-5-5".into()));
        assert_eq!(applied[1], Applied::ThinkingStarted);
        assert_eq!(applied[5], Applied::Text("Hello".into()));
        let result = acc.finish().unwrap();
        assert_eq!(result.text, "Hello there");
        assert_eq!(result.stop_reason, "end_turn");
        assert_eq!(
            result.content,
            json!([
                {"type": "thinking", "thinking": "", "signature": "sig=="},
                {"type": "text", "text": "Hello there"}
            ])
        );
    }

    #[test]
    fn refusals_and_truncated_streams_are_errors() {
        let mut acc = MessageAccumulator::default();
        feed(
            &mut acc,
            &[
                json!({"type": "message_start", "message": {"model": "m", "content": []}}),
                json!({"type": "message_delta", "delta": {"stop_reason": "refusal"}}),
                json!({"type": "message_stop"}),
            ],
        );
        assert_eq!(acc.finish().unwrap_err(), AiError::Refusal);

        let mut acc = MessageAccumulator::default();
        feed(
            &mut acc,
            &[json!({"type": "message_start", "message": {"model": "m", "content": []}})],
        );
        assert_eq!(acc.finish().unwrap_err(), AiError::Network);
    }

    #[test]
    fn stream_error_events_map_to_ai_errors() {
        let mut acc = MessageAccumulator::default();
        let err = acc
            .apply(&json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}))
            .unwrap_err();
        assert_eq!(err, AiError::Overloaded);
    }

    #[test]
    fn drops_internal_blocks_before_a_fallback() {
        let blocks = vec![
            json!({"type": "thinking", "thinking": "", "signature": "a"}),
            json!({"type": "text", "text": "Partial"}),
            json!({"type": "fallback", "from": {"model": "x"}, "to": {"model": "y"}}),
            json!({"type": "thinking", "thinking": "", "signature": "b"}),
            json!({"type": "text", "text": "Rest"}),
        ];
        let kept = sanitize_for_replay(blocks);
        assert_eq!(
            kept,
            vec![
                json!({"type": "text", "text": "Partial"}),
                json!({"type": "thinking", "thinking": "", "signature": "b"}),
                json!({"type": "text", "text": "Rest"}),
            ]
        );
        assert_eq!(visible_text(&kept), "Partial\n\nRest");
    }
}
