//! Renders a stored conversation as a single prompt for `claude -p`.
//!
//! Claude Code sessions are not persisted (`--no-session-persistence`), so
//! each request carries the conversation so far as a plain-text transcript,
//! followed by the app's instructions for this turn and the user's new words.

use crate::ai::{Turn, TurnRole};
use serde_json::Value;

/// The visible text of a stored turn (Anthropic-style content blocks or a string).
pub fn turn_text(turn: &Turn) -> String {
    match &turn.payload {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn tag(name: &str, text: &str) -> String {
    format!("<{name}>\n{}\n</{name}>\n", text.trim())
}

/// Turns `[history…, user, system*]` into one prompt.
pub fn render(turns: &[Turn]) -> String {
    let Some(last_user) = turns.iter().rposition(|t| t.role == TurnRole::User) else {
        return String::new();
    };
    let mut out = String::new();

    let history = &turns[..last_user];
    if !history.is_empty() {
        out.push_str(
            "This thought already has a conversation. Here it is, oldest first; your own earlier replies are marked <thoughtflow>.\n<conversation>\n",
        );
        for turn in history {
            let text = turn_text(turn);
            if text.trim().is_empty() {
                continue;
            }
            out.push_str(&match turn.role {
                TurnRole::User => tag("user", &text),
                TurnRole::Assistant => tag("thoughtflow", &text),
                TurnRole::System => tag("thoughtflow_mode", &text),
            });
        }
        out.push_str("</conversation>\n\n");
    }

    // App instructions for this turn (mode, date) come before the user's words.
    for turn in &turns[last_user + 1..] {
        if turn.role == TurnRole::System {
            out.push_str(&tag("thoughtflow_mode", &turn_text(turn)));
            out.push('\n');
        }
    }
    out.push_str(turn_text(&turns[last_user]).trim());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn turn(role: TurnRole, payload: Value) -> Turn {
        Turn { role, payload }
    }

    #[test]
    fn first_turn_is_mode_then_words() {
        let turns = [
            turn(
                TurnRole::User,
                json!([{"type": "text", "text": "<related_notes>n</related_notes>"}, {"type": "text", "text": "Finish physics"}]),
            ),
            turn(TurnRole::System, json!("Mode: Think. Today is Friday.")),
        ];
        assert_eq!(
            render(&turns),
            "<thoughtflow_mode>\nMode: Think. Today is Friday.\n</thoughtflow_mode>\n\n<related_notes>n</related_notes>\n\nFinish physics"
        );
    }

    #[test]
    fn later_turns_carry_a_transcript_without_reasoning_blocks() {
        let turns = [
            turn(TurnRole::User, json!([{"type": "text", "text": "first"}])),
            turn(TurnRole::System, json!("Mode: Think.")),
            turn(
                TurnRole::Assistant,
                json!([{"type": "thinking", "thinking": "", "signature": "s"}, {"type": "text", "text": "reply"}]),
            ),
            turn(TurnRole::User, json!([{"type": "text", "text": "second"}])),
            turn(TurnRole::System, json!("Mode: Plan.")),
        ];
        let prompt = render(&turns);
        assert!(prompt.starts_with("This thought already has a conversation."));
        assert!(prompt.contains("<user>\nfirst\n</user>\n<thoughtflow_mode>\nMode: Think.\n</thoughtflow_mode>\n<thoughtflow>\nreply\n</thoughtflow>\n</conversation>"));
        assert!(!prompt.contains("signature"));
        assert!(prompt.ends_with("<thoughtflow_mode>\nMode: Plan.\n</thoughtflow_mode>\n\nsecond"));
    }
}
