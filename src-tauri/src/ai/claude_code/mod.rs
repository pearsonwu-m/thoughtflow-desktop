//! Claude Code CLI provider: talks to Claude through the locally installed
//! `claude` command, using its sign-in (for example a Claude subscription)
//! instead of an API key.
//!
//! Each request runs `claude -p` in an empty working directory with Claude
//! Code's tools, MCP servers, plugins, hooks, skills and CLAUDE.md disabled
//! (`--tools "" --safe-mode --strict-mcp-config --disable-slash-commands`)
//! and without saving a session (`--no-session-persistence`). The prompt is
//! written to stdin, so thought text never appears in the process list.

mod events;
pub mod prompt;

use crate::ai::anthropic::caps::ModelCaps;
use crate::ai::{AiError, ChatOptions, ChatResult, StreamEvent, Turn};
use events::{map_error, parse_result, CliEvent, CliStream};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

/// Longest silence tolerated between output lines (startup plus thinking).
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const EXTRACTION_TIMEOUT: Duration = Duration::from_secs(240);

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeStatus {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub logged_in: bool,
    /// e.g. "claude.ai" (subscription) or "console" / "apiKey".
    pub auth_method: Option<String>,
    pub account: Option<String>,
    pub problem: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

/// The user's login-shell PATH. Apps opened from Finder get a minimal PATH,
/// which misses where `claude` (and, for npm installs, `node`) live.
fn login_shell_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut child = std::process::Command::new(shell)
            .args(["-lic", "printf '__TF_PATH__%s__TF_END__' \"$PATH\""])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < Duration::from_secs(4) => {
                    std::thread::sleep(Duration::from_millis(40))
                }
                _ => {
                    let _ = child.kill();
                    return None;
                }
            }
        }
        let mut out = String::new();
        std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
        let start = out.find("__TF_PATH__")? + "__TF_PATH__".len();
        let end = out[start..].find("__TF_END__")? + start;
        Some(out[start..end].to_string())
    })
    .as_deref()
}

fn child_path_env() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let extra = format!(
        "{}/.local/bin:/opt/homebrew/bin:/usr/local/bin",
        home().display()
    );
    match login_shell_path() {
        Some(login) => format!("{login}:{extra}"),
        None => format!("{current}:{extra}"),
    }
}

/// Finds the `claude` executable: an explicit path from Settings, the usual
/// install locations, then the login shell's PATH.
pub fn find_cli(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        let path = expand_tilde(p);
        return path.is_file().then_some(path);
    }
    let h = home();
    let candidates = [
        h.join(".local/bin/claude"),
        h.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    login_shell_path()?
        .split(':')
        .map(|dir| Path::new(dir).join("claude"))
        .find(|p| p.is_file())
}

pub struct ClaudeCodeClient {
    program: PathBuf,
    workdir: PathBuf,
}

impl ClaudeCodeClient {
    /// `workdir` is an empty folder owned by the app, so no project settings
    /// or CLAUDE.md from elsewhere are picked up.
    pub async fn locate(override_path: Option<String>, workdir: PathBuf) -> Result<Self, AiError> {
        let program = tokio::task::spawn_blocking(move || find_cli(override_path.as_deref()))
            .await
            .ok()
            .flatten()
            .ok_or(AiError::ClaudeCodeMissing)?;
        Ok(Self::with_program(program, workdir))
    }

    pub fn with_program(program: PathBuf, workdir: PathBuf) -> Self {
        Self { program, workdir }
    }

    async fn command(&self, args: &[String]) -> Result<Command, AiError> {
        tokio::fs::create_dir_all(&self.workdir)
            .await
            .map_err(|e| {
                AiError::ClaudeCode(format!("couldn't prepare its working folder ({e})."))
            })?;
        let path_env = tokio::task::spawn_blocking(child_path_env)
            .await
            .unwrap_or_default();
        let mut cmd = Command::new(&self.program);
        cmd.args(args)
            .current_dir(&self.workdir)
            .env("PATH", path_env)
            // Use Claude Code's own sign-in, not a key this app was given.
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(cmd)
    }

    fn base_args(opts: &ChatOptions, system_prompt: &str) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--model",
            &opts.model,
            "--system-prompt",
            system_prompt,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if ModelCaps::for_model(&opts.model).effort {
            args.extend(["--effort".to_string(), opts.effort.as_str().to_string()]);
        }
        args.extend(
            [
                "--tools",
                "",
                "--safe-mode",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--disable-slash-commands",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        args
    }

    async fn spawn(&self, args: &[String], prompt: &str) -> Result<Child, AiError> {
        let mut child = self
            .command(args)
            .await?
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => AiError::ClaudeCodeMissing,
                _ => AiError::ClaudeCode(format!("couldn't start ({e}).")),
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| AiError::ClaudeCode(format!("couldn't receive the prompt ({e}).")))?;
            // Dropping stdin closes it, which tells `claude -p` the prompt is complete.
        }
        Ok(child)
    }

    /// Streams a conversational reply.
    pub async fn stream_chat(
        &self,
        opts: &ChatOptions,
        system_prompt: &str,
        turns: &[Turn],
        cancel: &CancellationToken,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> Result<ChatResult, AiError> {
        on_event(StreamEvent::Sending {
            model: opts.model.clone(),
            via: "claude-code".into(),
        });
        let mut args = Self::base_args(opts, system_prompt);
        args.extend(
            [
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        let mut child = self.spawn(&args, &prompt::render(turns)).await?;
        let stderr = collect_stderr(&mut child);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AiError::ClaudeCode("produced no output.".into()))?;
        let mut lines = BufReader::new(stdout).lines();
        let mut stream = CliStream::default();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(AiError::Cancelled);
                }
                n = tokio::time::timeout(IDLE_TIMEOUT, lines.next_line()) => n,
            };
            let line = match next {
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(AiError::Timeout);
                }
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    return Err(AiError::ClaudeCode(format!(
                        "its output couldn't be read ({e})."
                    )))
                }
            };
            match stream.apply_line(&line) {
                CliEvent::Started(model) => on_event(StreamEvent::Started { model }),
                CliEvent::Thinking => on_event(StreamEvent::Thinking),
                CliEvent::Text(text) => on_event(StreamEvent::Delta { text }),
                CliEvent::Nothing => {}
            }
        }
        let _ = child.wait().await;
        if !stream.has_result() {
            return Err(failure_from_stderr(stderr.await.unwrap_or_default()));
        }
        stream.finish(&opts.model)
    }

    /// A one-shot request whose answer must match `schema` (via `--json-schema`).
    pub async fn complete_json(
        &self,
        opts: &ChatOptions,
        system_prompt: &str,
        user_text: &str,
        schema: &Value,
        cancel: &CancellationToken,
    ) -> Result<Value, AiError> {
        let mut args = Self::base_args(opts, system_prompt);
        args.extend([
            "--output-format".into(),
            "json".into(),
            "--json-schema".into(),
            schema.to_string(),
        ]);
        let mut child = self.spawn(&args, user_text).await?;
        let stderr = collect_stderr(&mut child);
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| AiError::ClaudeCode("produced no output.".into()))?;
        let mut out = String::new();
        let read = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(AiError::Cancelled);
            }
            r = tokio::time::timeout(EXTRACTION_TIMEOUT, stdout.read_to_string(&mut out)) => r,
        };
        if read.is_err() {
            let _ = child.kill().await;
            return Err(AiError::Timeout);
        }
        let _ = child.wait().await;
        let Ok(v) = serde_json::from_str::<Value>(out.trim()) else {
            return Err(failure_from_stderr(stderr.await.unwrap_or_default()));
        };
        let result = parse_result(&v);
        if result.is_error {
            return Err(map_error(None, &result.text, &opts.model));
        }
        match result.structured {
            Some(structured) => Ok(structured),
            None => crate::ai::anthropic::parse_json_object(&result.text),
        }
    }

    /// Version and sign-in state. Costs nothing: no prompt is sent.
    pub async fn status(override_path: Option<String>) -> ClaudeCodeStatus {
        let Some(program) = tokio::task::spawn_blocking(move || find_cli(override_path.as_deref()))
            .await
            .ok()
            .flatten()
        else {
            return ClaudeCodeStatus {
                problem: Some(
                    "Claude Code wasn't found. Install it, or enter the path to `claude`.".into(),
                ),
                ..Default::default()
            };
        };
        let mut status = ClaudeCodeStatus {
            found: true,
            path: Some(program.display().to_string()),
            ..Default::default()
        };
        let client = Self::with_program(program, std::env::temp_dir());
        let run = |args: &'static [&'static str]| {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let client = &client;
            async move {
                let mut cmd = client.command(&args).await.ok()?;
                cmd.stdin(Stdio::null());
                let output = tokio::time::timeout(Duration::from_secs(20), cmd.output())
                    .await
                    .ok()?
                    .ok()?;
                Some(String::from_utf8_lossy(&output.stdout).to_string())
            }
        };
        status.version = run(&["--version"])
            .await
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        match run(&["auth", "status"])
            .await
            .and_then(|out| serde_json::from_str::<Value>(out.trim()).ok())
        {
            Some(auth) => {
                status.logged_in = auth["loggedIn"].as_bool().unwrap_or(false);
                status.auth_method = auth["authMethod"].as_str().map(str::to_string);
                status.account = auth["email"].as_str().map(str::to_string);
                if !status.logged_in {
                    status.problem = Some(
                        "Claude Code isn't signed in. Run `claude` in Terminal to sign in.".into(),
                    );
                }
            }
            None => status.problem = Some("Couldn't read Claude Code's sign-in status.".into()),
        }
        status
    }
}

/// Reads stderr in the background so a chatty process can't block on a full pipe.
fn collect_stderr(child: &mut Child) -> tokio::task::JoinHandle<String> {
    let stderr = child.stderr.take();
    tokio::spawn(async move {
        let mut text = String::new();
        if let Some(mut s) = stderr {
            let _ = s.read_to_string(&mut text).await;
        }
        text
    })
}

fn failure_from_stderr(stderr: String) -> AiError {
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if first.is_empty() {
        AiError::ClaudeCode("it exited without replying.".into())
    } else {
        map_error(None, first, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Effort, TurnRole};
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    fn opts() -> ChatOptions {
        ChatOptions {
            model: "claude-opus-5-5".into(),
            effort: Effort::Low,
            temperature: None,
            max_tokens: 1000,
        }
    }

    /// A stand-in `claude` executable: records its stdin and arguments, then
    /// prints canned output.
    fn fake_cli(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("claude");
        let script = format!(
            "#!/bin/sh\ncat > \"{dir}/stdin.txt\"\necho \"$@\" > \"{dir}/args.txt\"\nenv > \"{dir}/env.txt\"\n{body}\n",
            dir = dir.display()
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tf-cli-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn streams_through_a_cli_process_and_keeps_thoughts_off_the_command_line() {
        let dir = temp_dir();
        let lines = [
            r#"{"type":"system","subtype":"init","model":"claude-opus-5-5"}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"model":"claude-opus-5-5","content":[]}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"What I'm hearing"}}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"What I'm hearing","stop_reason":"end_turn"}"#,
        ];
        // A quoted heredoc prints the lines verbatim (the reply contains an apostrophe).
        let body = format!("cat <<'TF_EOF'\n{}\nTF_EOF", lines.join("\n"));
        let program = fake_cli(&dir, &body);
        let client = ClaudeCodeClient::with_program(program, dir.join("work"));
        let turns = [Turn {
            role: TurnRole::User,
            payload: json!([{"type": "text", "text": "a private thought"}]),
        }];
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let on_event = move |e: StreamEvent| sink.lock().unwrap().push(e);
        let result = client
            .stream_chat(&opts(), "SYS", &turns, &CancellationToken::new(), &on_event)
            .await
            .unwrap();

        assert_eq!(result.text, "What I'm hearing");
        let events = events.lock().unwrap();
        assert!(matches!(&events[0], StreamEvent::Sending { via, .. } if via == "claude-code"));
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::Delta { text } if text == "What I'm hearing")));

        let stdin = std::fs::read_to_string(dir.join("stdin.txt")).unwrap();
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        let env = std::fs::read_to_string(dir.join("env.txt")).unwrap();
        assert_eq!(stdin, "a private thought");
        assert!(
            !args.contains("private thought"),
            "thought text must not be an argument"
        );
        for flag in [
            "-p",
            "--safe-mode",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--effort low",
            "--model claude-opus-5-5",
        ] {
            assert!(args.contains(flag), "missing {flag}: {args}");
        }
        assert!(!env.lines().any(|l| l.starts_with("ANTHROPIC_API_KEY=")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn reports_sign_in_problems_and_missing_cli() {
        let dir = temp_dir();
        let program = fake_cli(
            &dir,
            r#"echo '{"type":"assistant","error":"authentication_failed","message":{"content":[]}}'
echo '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}'"#,
        );
        let client = ClaudeCodeClient::with_program(program, dir.join("work"));
        let turns = [Turn {
            role: TurnRole::User,
            payload: json!("hi"),
        }];
        let err = client
            .stream_chat(&opts(), "SYS", &turns, &CancellationToken::new(), &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err, AiError::ClaudeCodeSignedOut);

        let missing = ClaudeCodeClient::with_program(dir.join("nope"), dir.join("work"));
        let err = missing
            .stream_chat(&opts(), "SYS", &turns, &CancellationToken::new(), &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err, AiError::ClaudeCodeMissing);
        assert!(find_cli(Some(dir.join("nope").to_str().unwrap())).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn stop_kills_the_process() {
        let dir = temp_dir();
        let program = fake_cli(&dir, "sleep 30");
        let client = ClaudeCodeClient::with_program(program, dir.join("work"));
        let turns = [Turn {
            role: TurnRole::User,
            payload: json!("hi"),
        }];
        let cancel = CancellationToken::new();
        let stopper = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            stopper.cancel();
        });
        let started = std::time::Instant::now();
        let err = client
            .stream_chat(&opts(), "SYS", &turns, &cancel, &|_| {})
            .await
            .unwrap_err();
        assert_eq!(err, AiError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn extracts_structured_output() {
        let dir = temp_dir();
        let program = fake_cli(
            &dir,
            r#"echo '{"type":"result","subtype":"success","is_error":false,"result":"{}","structured_output":{"tasks":[{"name":"Email Dr. Lignos"}]}}'"#,
        );
        let client = ClaudeCodeClient::with_program(program, dir.join("work"));
        let schema = json!({"type": "object"});
        let value = client
            .complete_json(
                &opts(),
                "SYS",
                "extract",
                &schema,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(value["tasks"][0]["name"], "Email Dr. Lignos");
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        assert!(args.contains("--json-schema"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Uses the real `claude` CLI. Run with `cargo test live_claude_code -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "runs the local Claude Code CLI and uses a little of its usage"]
    async fn live_claude_code_round_trip() {
        let status = ClaudeCodeClient::status(None).await;
        eprintln!("{status:?}");
        assert!(status.found && status.logged_in, "{status:?}");
        let workdir = std::env::temp_dir().join("thoughtflow-claude-code-test");
        let client = ClaudeCodeClient::locate(None, workdir).await.unwrap();
        let turns = [
            Turn {
                role: TurnRole::User,
                payload: json!([{"type": "text", "text": "I need to finish my physics work and email Dr. Lignos."}]),
            },
            Turn {
                role: TurnRole::System,
                payload: json!("Mode: Think. This is the start of a new thought."),
            },
        ];
        let result = client
            .stream_chat(
                &opts(),
                crate::ai::prompts::SYSTEM_PROMPT,
                &turns,
                &CancellationToken::new(),
                &|_| {},
            )
            .await
            .unwrap();
        eprintln!("--- reply ({}) ---\n{}", result.model, result.text);
        assert!(!result.text.is_empty());
        let plan = client
            .complete_json(
                &opts(),
                crate::ai::prompts::EXTRACTION_SYSTEM_PROMPT,
                &crate::ai::prompts::tasks_extraction_request("2026-10-02", &format!("<user>\nfinish physics, email Dr. Lignos\n</user>\n<thoughtflow>\n{}\n</thoughtflow>\n", result.text)),
                &crate::ai::prompts::tasks_schema(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        eprintln!("--- tasks ---\n{plan:#}");
        assert!(plan["tasks"].as_array().is_some_and(|t| !t.is_empty()));
    }
}
