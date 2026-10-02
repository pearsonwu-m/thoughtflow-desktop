//! Maps Anthropic API failures to [`AiError`].

use crate::ai::AiError;
use serde_json::Value;

/// Classifies an error body (`{"type":"error","error":{"type":..,"message":..}}`)
/// together with its HTTP status (0 for errors delivered inside a stream).
pub fn error_from_body(body: &Value, status: u16, model: &str) -> AiError {
    let kind = body["error"]["type"].as_str().unwrap_or_default();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    match (status, kind) {
        (401, _) | (_, "authentication_error") => AiError::InvalidApiKey,
        (402, _) | (_, "billing_error") => AiError::Billing,
        (403, _) | (_, "permission_error") => AiError::PermissionDenied,
        (404, _) | (_, "not_found_error") => AiError::ModelNotFound(model.to_string()),
        (413, _) | (_, "request_too_large") => AiError::TooLarge,
        (429, _) | (_, "rate_limit_error") => AiError::RateLimited {
            retry_after_secs: None,
        },
        (529, _) | (_, "overloaded_error") => AiError::Overloaded,
        (_, "api_error") => AiError::Server(if status == 0 { 500 } else { status }),
        (s, _) if s >= 500 => AiError::Server(s),
        (_, "invalid_request_error") | (400, _) => {
            if message.contains("prompt is too long") || message.contains("too many tokens") {
                AiError::TooLarge
            } else {
                AiError::BadRequest(short(message))
            }
        }
        _ if message.is_empty() => AiError::Malformed(format!("unexpected error (HTTP {status})")),
        _ => AiError::BadRequest(short(message)),
    }
}

fn short(message: &str) -> String {
    let m = message.trim();
    if m.is_empty() {
        return "the API rejected the request.".to_string();
    }
    crate::util::truncate_chars(m, 220)
}

pub fn from_reqwest(err: &reqwest::Error) -> AiError {
    if err.is_timeout() {
        AiError::Timeout
    } else {
        AiError::Network
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(kind: &str, message: &str) -> Value {
        json!({"type": "error", "error": {"type": kind, "message": message}})
    }

    #[test]
    fn maps_statuses_to_human_errors() {
        assert_eq!(
            error_from_body(&body("authentication_error", "invalid x-api-key"), 401, "m"),
            AiError::InvalidApiKey
        );
        assert_eq!(
            error_from_body(&body("not_found_error", "model: nope"), 404, "nope"),
            AiError::ModelNotFound("nope".into())
        );
        assert_eq!(
            error_from_body(&body("rate_limit_error", ""), 429, "m"),
            AiError::RateLimited {
                retry_after_secs: None
            }
        );
        assert_eq!(
            error_from_body(&body("overloaded_error", ""), 529, "m"),
            AiError::Overloaded
        );
        assert_eq!(
            error_from_body(&body("api_error", ""), 500, "m"),
            AiError::Server(500)
        );
        assert_eq!(
            error_from_body(
                &body(
                    "invalid_request_error",
                    "prompt is too long: 1200000 tokens"
                ),
                400,
                "m"
            ),
            AiError::TooLarge
        );
        assert_eq!(
            error_from_body(
                &body("invalid_request_error", "max_tokens: too big"),
                400,
                "m"
            ),
            AiError::BadRequest("max_tokens: too big".into())
        );
        assert!(matches!(
            error_from_body(&json!("not json"), 418, "m"),
            AiError::Malformed(_)
        ));
    }
}
