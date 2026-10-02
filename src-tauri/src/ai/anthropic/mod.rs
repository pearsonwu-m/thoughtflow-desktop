//! Anthropic Messages API client.
//!
//! There is no official Anthropic SDK for Rust, so this module talks to the
//! REST API directly. It is the only place in the app that knows about
//! Anthropic's wire format; the API key never leaves the Rust process.

pub(crate) mod accumulate;
pub mod caps;
mod errors;
pub mod request;
mod sse;

use crate::ai::{AiError, ChatResult, ModelInfo, StreamEvent};
use accumulate::{Applied, MessageAccumulator};
use errors::{error_from_body, from_reqwest};
use futures_util::StreamExt;
use request::BuiltRequest;
use serde_json::Value;
use sse::SseParser;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub use accumulate::visible_text;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const MAX_RETRIES: u32 = 2;
/// Longest silence tolerated mid-stream. The API sends periodic pings, so a
/// gap this long means the connection is gone.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("Thoughtflow/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        // Overridable for tests and for users routing through a proxy.
        let base_url = std::env::var("THOUGHTFLOW_API_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(http, api_key, base_url)
    }

    pub fn with_base_url(http: reqwest::Client, api_key: String, base_url: String) -> Self {
        Self {
            http,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn post_messages(&self, req: &BuiltRequest) -> reqwest::RequestBuilder {
        let mut builder = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");
        if !req.betas.is_empty() {
            builder = builder.header("anthropic-beta", req.betas.join(","));
        }
        builder.body(req.body.to_string())
    }

    fn model_of(req: &BuiltRequest) -> String {
        req.body["model"].as_str().unwrap_or_default().to_string()
    }

    /// Sends `req`, retrying transient failures that happen before any output
    /// arrives. Returns the successful HTTP response.
    async fn send_with_retries(
        &self,
        req: &BuiltRequest,
        timeout: Option<Duration>,
        cancel: &CancellationToken,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> Result<reqwest::Response, AiError> {
        let model = Self::model_of(req);
        let mut attempt = 0;
        loop {
            let mut builder = self.post_messages(req);
            if let Some(t) = timeout {
                builder = builder.timeout(t);
            }
            let outcome = tokio::select! {
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                r = builder.send() => r,
            };
            let (err, retry_after) = match outcome {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.trim().parse::<u64>().ok());
                    let body: Value = resp.json().await.unwrap_or(Value::Null);
                    let mut err = error_from_body(&body, status, &model);
                    if let AiError::RateLimited { retry_after_secs } = &mut err {
                        *retry_after_secs = retry_after;
                    }
                    (err, retry_after)
                }
                Err(e) => (from_reqwest(&e), None),
            };
            if !err.retryable() || attempt >= MAX_RETRIES {
                return Err(err);
            }
            attempt += 1;
            let delay = retry_delay(attempt, retry_after);
            // Long server-requested waits are better reported than hidden.
            if delay > Duration::from_secs(20) {
                return Err(err);
            }
            on_event(StreamEvent::Retrying {
                attempt,
                delay_ms: delay.as_millis() as u64,
                reason: err.kind().to_string(),
            });
            tokio::select! {
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// Streams a reply, forwarding visible text as it arrives.
    pub async fn stream_message(
        &self,
        req: &BuiltRequest,
        cancel: &CancellationToken,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> Result<ChatResult, AiError> {
        on_event(StreamEvent::Sending {
            model: Self::model_of(req),
            via: "api".into(),
        });
        let resp = self.send_with_retries(req, None, cancel, on_event).await?;
        let mut stream = resp.bytes_stream();
        let mut parser = SseParser::default();
        let mut acc = MessageAccumulator::default();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                n = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => n,
            };
            let chunk = match next {
                Err(_) => return Err(AiError::Timeout),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(from_reqwest(&e)),
                Ok(Some(Ok(bytes))) => bytes,
            };
            for event in parser.feed(&chunk) {
                if event.data.is_empty() {
                    continue;
                }
                let json: Value = serde_json::from_str(&event.data)
                    .map_err(|_| AiError::Malformed("invalid stream event".into()))?;
                match acc.apply(&json)? {
                    Applied::Started(model) => on_event(StreamEvent::Started { model }),
                    Applied::ThinkingStarted => on_event(StreamEvent::Thinking),
                    Applied::Text(text) => on_event(StreamEvent::Delta { text }),
                    Applied::Stopped | Applied::Nothing => {}
                }
            }
        }
        acc.finish()
    }

    /// Sends a non-streaming request and returns the reply's visible text.
    pub async fn complete_text(
        &self,
        req: &BuiltRequest,
        cancel: &CancellationToken,
    ) -> Result<String, AiError> {
        let resp = self
            .send_with_retries(req, Some(REQUEST_TIMEOUT), cancel, &|_| {})
            .await?;
        let body: Value = resp
            .json()
            .await
            .map_err(|_| AiError::Malformed("invalid JSON body".into()))?;
        match body["stop_reason"].as_str() {
            Some("refusal") => return Err(AiError::Refusal),
            Some("max_tokens") => return Err(AiError::Malformed("response was cut off".into())),
            _ => {}
        }
        let content = body["content"].as_array().cloned().unwrap_or_default();
        let text = visible_text(&content);
        if text.is_empty() {
            return Err(AiError::Malformed("empty response".into()));
        }
        Ok(text)
    }

    /// Lists models available to the key. Used as a cheap connection test.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, AiError> {
        let resp = self
            .http
            .get(format!("{}/v1/models?limit=100", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| from_reqwest(&e))?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            return Err(error_from_body(&body, status, ""));
        }
        let models = body["data"]
            .as_array()
            .ok_or_else(|| AiError::Malformed("missing model list".into()))?
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                let display_name = m["display_name"].as_str().unwrap_or(&id).to_string();
                Some(ModelInfo { id, display_name })
            })
            .collect();
        Ok(models)
    }
}

fn retry_delay(attempt: u32, retry_after_secs: Option<u64>) -> Duration {
    if let Some(secs) = retry_after_secs {
        return Duration::from_secs(secs.max(1));
    }
    let base = 800u64 * 2u64.pow(attempt.saturating_sub(1));
    let jitter = (crate::util::now_ms() as u64) % 250;
    Duration::from_millis(base + jitter)
}

/// Pulls the first JSON object out of model text, tolerating code fences or
/// stray prose around it (used when structured outputs are unavailable).
pub fn parse_json_object(text: &str) -> Result<Value, AiError> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Ok(v);
        }
    }
    let start = trimmed.find('{');
    let end = trimmed.rfind('}');
    match (start, end) {
        (Some(s), Some(e)) if e > s => serde_json::from_str(&trimmed[s..=e])
            .map_err(|_| AiError::Malformed("response was not valid JSON".into())),
        _ => Err(AiError::Malformed("response contained no JSON".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::anthropic::caps::ModelCaps;
    use crate::ai::{ChatOptions, Effort, Turn, TurnRole};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves canned HTTP responses, one per connection, in order.
    async fn mock_server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 64 * 1024];
                let mut received = Vec::new();
                // Read until the full body (per Content-Length) has arrived.
                loop {
                    let n = socket.read(&mut buf).await.unwrap();
                    received.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&received).to_string();
                    if let Some(idx) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if received.len() >= idx + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&received).to_string());
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.ok();
            }
        });
        (format!("http://{addr}"), requests)
    }

    fn http(status: &str, content_type: &str, extra: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
            body.len()
        )
    }

    fn sse(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e))
            .collect()
    }

    fn request() -> BuiltRequest {
        let caps = ModelCaps::for_model("claude-opus-5-5");
        let opts = ChatOptions {
            model: "claude-opus-5-5".into(),
            effort: Effort::Low,
            temperature: None,
            max_tokens: 1024,
        };
        let turns = [Turn {
            role: TurnRole::User,
            payload: json!([{"type": "text", "text": "hi"}]),
        }];
        request::chat_request(&opts, &caps, "SYS", &turns)
    }

    #[tokio::test]
    async fn streams_a_reply_after_retrying_an_overload() {
        let stream_body = sse(&[
            json!({"type": "message_start", "message": {"id": "msg_1", "model": "claude-opus-5-5", "content": [], "usage": {}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "abc"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "What I'm hearing"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 3}}),
            json!({"type": "message_stop"}),
        ]);
        let overloaded = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}).to_string();
        let (base, requests) = mock_server(vec![
            http(
                "529 Overloaded",
                "application/json",
                "retry-after: 1\r\n",
                &overloaded,
            ),
            http("200 OK", "text/event-stream", "", &stream_body),
        ])
        .await;

        let client = AnthropicClient::with_base_url(http_client(), "sk-ant-test".into(), base);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let on_event = move |e: StreamEvent| sink.lock().unwrap().push(e);
        let result = client
            .stream_message(&request(), &CancellationToken::new(), &on_event)
            .await
            .unwrap();

        assert_eq!(result.text, "What I'm hearing");
        assert_eq!(result.content[0]["signature"], "abc");
        let events = events.lock().unwrap();
        assert!(matches!(events[0], StreamEvent::Sending { .. }));
        assert!(matches!(
            events[1],
            StreamEvent::Retrying { attempt: 1, .. }
        ));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Thinking)));

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let first = requests[0].to_ascii_lowercase();
        assert!(first.contains("x-api-key: sk-ant-test"));
        assert!(first.contains("anthropic-version: 2023-06-01"));
        assert!(first.contains("anthropic-beta: server-side-fallback-2026-07-01"));
    }

    #[tokio::test]
    async fn invalid_keys_fail_without_retrying() {
        let body = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}}).to_string();
        let (base, requests) = mock_server(vec![http(
            "401 Unauthorized",
            "application/json",
            "",
            &body,
        )])
        .await;
        let client = AnthropicClient::with_base_url(http_client(), "sk-ant-bad".into(), base);
        let err = client
            .stream_message(&request(), &CancellationToken::new(), &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err, AiError::InvalidApiKey);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unreachable_hosts_report_a_network_error() {
        // Bind then drop a listener so the port is closed.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let client =
            AnthropicClient::with_base_url(http_client(), "k".into(), format!("http://{addr}"));
        let cancel = CancellationToken::new();
        let err = client.list_models().await.unwrap_err();
        assert_eq!(err, AiError::Network);
        cancel.cancel();
        let err = client
            .stream_message(&request(), &cancel, &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err, AiError::Cancelled);
    }

    #[tokio::test]
    async fn lists_models() {
        let body = json!({"data": [{"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5"}], "has_more": false}).to_string();
        let (base, _) = mock_server(vec![http("200 OK", "application/json", "", &body)]).await;
        let client = AnthropicClient::with_base_url(http_client(), "k".into(), base);
        let models = client.list_models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name, "Claude Opus 5.5");
    }

    /// Talks to the real API. Run with `cargo test -- --ignored --nocapture`.
    /// Without ANTHROPIC_API_KEY it only checks that a bad key is reported as such;
    /// with a key it runs a two-turn Think conversation (replaying the first
    /// reply verbatim, thinking blocks included) and a structured plan extraction.
    #[tokio::test]
    #[ignore = "network: calls api.anthropic.com"]
    async fn live_api_round_trip() {
        use crate::ai::prompts;
        let bad = AnthropicClient::with_base_url(
            http_client(),
            "sk-ant-api03-invalid-key-for-testing".into(),
            DEFAULT_BASE_URL.into(),
        );
        assert_eq!(bad.list_models().await.unwrap_err(), AiError::InvalidApiKey);
        eprintln!("invalid key correctly rejected");

        let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else {
            eprintln!("ANTHROPIC_API_KEY not set; skipped the authenticated checks");
            return;
        };
        let model =
            std::env::var("THOUGHTFLOW_TEST_MODEL").unwrap_or_else(|_| "claude-opus-5-5".into());
        let client = AnthropicClient::with_base_url(http_client(), key, DEFAULT_BASE_URL.into());
        let models = client.list_models().await.expect("list models");
        eprintln!("{} models available", models.len());

        let caps = ModelCaps::for_model(&model);
        let opts = ChatOptions {
            model: model.clone(),
            effort: Effort::Low,
            temperature: None,
            max_tokens: 4000,
        };
        let mut turns = vec![
            Turn {
                role: TurnRole::User,
                payload: prompts::user_content(
                    &[],
                    "I need to finish my physics work, email Dr. Lignos, and work on my website.",
                ),
            },
            Turn {
                role: TurnRole::System,
                payload: json!(prompts::mode_message(
                    crate::ai::Mode::Think,
                    "Thursday, October 1, 2026",
                    true
                )),
            },
        ];
        let first = client
            .stream_message(
                &request::chat_request(&opts, &caps, prompts::SYSTEM_PROMPT, &turns),
                &CancellationToken::new(),
                &|_| {},
            )
            .await
            .expect("first turn");
        eprintln!("--- first reply ({}) ---\n{}", first.model, first.text);
        assert!(!first.text.is_empty());

        turns.push(Turn {
            role: TurnRole::Assistant,
            payload: first.content.clone(),
        });
        turns.push(Turn {
            role: TurnRole::User,
            payload: prompts::user_content(&[], "Physics is due Friday."),
        });
        turns.push(Turn {
            role: TurnRole::System,
            payload: json!(prompts::mode_message(
                crate::ai::Mode::Plan,
                "Thursday, October 1, 2026",
                false
            )),
        });
        let second = client
            .stream_message(
                &request::chat_request(&opts, &caps, prompts::SYSTEM_PROMPT, &turns),
                &CancellationToken::new(),
                &|_| {},
            )
            .await
            .expect("second turn replays the first verbatim");
        eprintln!("--- second reply ---\n{}", second.text);
        assert!(!second.text.is_empty());

        let transcript = prompts::transcript(&[
            ("user".into(), "Finish physics by Friday".into()),
            ("thoughtflow".into(), second.text.clone()),
        ]);
        let extraction = request::extraction_request(
            &opts,
            &caps,
            prompts::EXTRACTION_SYSTEM_PROMPT,
            &prompts::plan_extraction_request("2026-10-01", &transcript),
            &prompts::plan_schema(),
        );
        let text = client
            .complete_text(&extraction, &CancellationToken::new())
            .await
            .expect("extraction");
        let plan = parse_json_object(&text).expect("plan JSON");
        eprintln!("--- extracted plan ---\n{plan:#}");
        assert!(plan["steps"].as_array().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn parses_json_with_surrounding_noise() {
        assert_eq!(parse_json_object("{\"a\":1}").unwrap(), json!({"a": 1}));
        assert_eq!(
            parse_json_object("```json\n{\"a\":1}\n```").unwrap(),
            json!({"a": 1})
        );
        assert!(parse_json_object("nothing here").is_err());
    }
}
