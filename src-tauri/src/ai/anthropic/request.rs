//! Builds Messages API request bodies from stored turns.

use super::caps::ModelCaps;
use crate::ai::{ChatOptions, Turn, TurnRole};
use serde_json::{json, Value};

/// Beta header for the scalar `fallbacks: "default"` refusal fallback.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Debug, Clone)]
pub struct BuiltRequest {
    pub body: Value,
    pub betas: Vec<&'static str>,
}

/// A conversational request. The system prompt is sent unchanged on every
/// request and turns are replayed verbatim; only cache markers are added here.
pub fn chat_request(
    opts: &ChatOptions,
    caps: &ModelCaps,
    system_prompt: &str,
    turns: &[Turn],
) -> BuiltRequest {
    let mut body = json!({
        "model": opts.model,
        "max_tokens": opts.max_tokens,
        "stream": true,
        "system": [{"type": "text", "text": system_prompt, "cache_control": {"type": "ephemeral"}}],
        "messages": render_messages(turns, caps),
    });
    let mut betas = Vec::new();
    apply_common(&mut body, &mut betas, opts, caps);
    BuiltRequest { body, betas }
}

/// A one-shot request that returns JSON matching `schema`.
pub fn extraction_request(
    opts: &ChatOptions,
    caps: &ModelCaps,
    system_prompt: &str,
    user_text: &str,
    schema: &Value,
) -> BuiltRequest {
    let text = if caps.structured_outputs {
        user_text.to_string()
    } else {
        format!(
            "{user_text}{}",
            crate::ai::prompts::schema_instruction(schema)
        )
    };
    let mut body = json!({
        "model": opts.model,
        "max_tokens": opts.max_tokens,
        "system": system_prompt,
        "messages": [{"role": "user", "content": text}],
    });
    let mut betas = Vec::new();
    apply_common(&mut body, &mut betas, opts, caps);
    if caps.structured_outputs {
        body["output_config"]["format"] = json!({"type": "json_schema", "schema": schema});
    }
    BuiltRequest { body, betas }
}

fn apply_common(
    body: &mut Value,
    betas: &mut Vec<&'static str>,
    opts: &ChatOptions,
    caps: &ModelCaps,
) {
    if caps.effort {
        body["output_config"] = json!({"effort": opts.effort.as_str()});
    }
    if caps.sampling {
        if let Some(t) = opts.temperature {
            body["temperature"] = json!(t.clamp(0.0, 1.0));
        }
    }
    if caps.server_fallbacks {
        body["fallbacks"] = json!("default");
        betas.push(FALLBACK_BETA);
    }
}

/// Converts stored turns into API messages.
///
/// App instructions are stored as system turns. Models that accept
/// mid-conversation system messages get them as-is; for other models the text
/// is attached to the preceding user message so the request stays valid.
pub fn render_messages(turns: &[Turn], caps: &ModelCaps) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(turns.len());
    for turn in turns {
        match turn.role {
            TurnRole::User => {
                out.push(json!({"role": "user", "content": as_blocks(&turn.payload)}))
            }
            TurnRole::Assistant => {
                out.push(json!({"role": "assistant", "content": turn.payload.clone()}))
            }
            TurnRole::System => {
                let text = turn.payload.as_str().unwrap_or_default();
                if caps.system_messages {
                    out.push(json!({"role": "system", "content": text}));
                } else if let Some(last) = out.last_mut().filter(|m| m["role"] == "user") {
                    if let Some(blocks) = last["content"].as_array_mut() {
                        blocks.push(json!({
                            "type": "text",
                            "text": format!("<thoughtflow_mode>\n{text}\n</thoughtflow_mode>"),
                        }));
                    }
                }
            }
        }
    }
    // Cache the conversation up to the newest user turn. Markers are not part
    // of the stored history, so adding them never edits earlier turns.
    if let Some(last_user) = out.iter_mut().rev().find(|m| m["role"] == "user") {
        if let Some(block) = last_user["content"]
            .as_array_mut()
            .and_then(|b| b.last_mut())
        {
            block["cache_control"] = json!({"type": "ephemeral"});
        }
    }
    out
}

fn as_blocks(payload: &Value) -> Value {
    match payload {
        Value::String(s) => json!([{"type": "text", "text": s}]),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Effort;

    fn opts(model: &str) -> ChatOptions {
        ChatOptions {
            model: model.into(),
            effort: Effort::Low,
            temperature: Some(0.7),
            max_tokens: 16000,
        }
    }

    fn turns() -> Vec<Turn> {
        vec![
            Turn {
                role: TurnRole::User,
                payload: json!([{"type": "text", "text": "first"}]),
            },
            Turn {
                role: TurnRole::System,
                payload: json!("Mode: Think."),
            },
            Turn {
                role: TurnRole::Assistant,
                payload: json!([{"type": "thinking", "thinking": "", "signature": "s"}, {"type": "text", "text": "reply"}]),
            },
            Turn {
                role: TurnRole::User,
                payload: json!([{"type": "text", "text": "second"}]),
            },
        ]
    }

    #[test]
    fn opus_requests_use_system_messages_effort_and_fallbacks() {
        let caps = ModelCaps::for_model("claude-opus-5-5");
        let req = chat_request(&opts("claude-opus-5-5"), &caps, "SYS", &turns());
        let body = &req.body;
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["fallbacks"], "default");
        assert!(
            body.get("temperature").is_none(),
            "Opus 5.5 rejects temperature"
        );
        assert!(
            body.get("thinking").is_none(),
            "thinking stays at the model default"
        );
        assert_eq!(req.betas, vec![FALLBACK_BETA]);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages[1],
            json!({"role": "system", "content": "Mode: Think."})
        );
        // Assistant turns are replayed verbatim, thinking signature included.
        assert_eq!(messages[2]["content"][0]["signature"], "s");
        // Only the newest user turn carries the cache marker.
        assert!(messages[0]["content"][0].get("cache_control").is_none());
        assert_eq!(
            messages[3]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn haiku_requests_fold_mode_text_into_the_user_turn() {
        let caps = ModelCaps::for_model("claude-haiku-4-5");
        let req = chat_request(&opts("claude-haiku-4-5"), &caps, "SYS", &turns());
        let body = &req.body;
        assert!(body.get("output_config").is_none());
        assert!(body.get("fallbacks").is_none());
        assert!((body["temperature"].as_f64().unwrap() - 0.7).abs() < 1e-6);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0]["content"][1]["text"],
            "<thoughtflow_mode>\nMode: Think.\n</thoughtflow_mode>"
        );
        assert!(req.betas.is_empty());
    }

    #[test]
    fn extraction_uses_structured_outputs_when_available() {
        let schema = json!({"type": "object", "additionalProperties": false, "properties": {}, "required": []});
        let caps = ModelCaps::for_model("claude-opus-5-5");
        let req = extraction_request(&opts("claude-opus-5-5"), &caps, "SYS", "extract", &schema);
        assert_eq!(req.body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(req.body["output_config"]["effort"], "low");
        assert!(req.body.get("stream").is_none());

        let caps = ModelCaps::for_model("claude-3-5-sonnet-20241022");
        let req = extraction_request(
            &opts("claude-3-5-sonnet-20241022"),
            &caps,
            "SYS",
            "extract",
            &schema,
        );
        assert!(req.body.get("output_config").is_none());
        assert!(req.body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("JSON Schema"));
    }
}
