//! Small, dependency-free helpers shared across modules.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// UTC calendar date (YYYY-MM-DD) for a Unix-millisecond timestamp.
/// Uses Howard Hinnant's `civil_from_days` so we don't need a date crate.
pub fn ymd_utc(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Truncate to at most `max` characters, appending an ellipsis when cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    // Prefer to break on a word boundary if one is reasonably close.
    let trimmed = match cut.rfind(char::is_whitespace) {
        Some(i) if i > max / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!(
        "{}…",
        trimmed.trim_end_matches(|c: char| c.is_whitespace() || ",;:-".contains(c))
    )
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A short, human title derived from a raw thought: the first sentence or line,
/// clipped to a notebook-friendly length.
pub fn derive_title(raw: &str) -> String {
    let first_line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = collapse_whitespace(first_line);
    // Stop at the end of the first sentence when there is one.
    let sentence = line
        .char_indices()
        .find(|&(i, c)| matches!(c, '.' | '?' | '!' | '。' | '？' | '！') && i > 12)
        .map(|(i, _)| &line[..i])
        .unwrap_or(&line);
    let title = truncate_chars(sentence, 56);
    let title = title.trim_end_matches(['.', ',', ';', ':']);
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Untitled thought".to_string(),
    }
}

/// Strip the most common Markdown markers so text reads cleanly as a summary.
pub fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let l = line.trim_start_matches(['#', '>', ' ']).trim();
        let l = l
            .strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .unwrap_or(l);
        out.push_str(&l.replace("**", "").replace('`', ""));
        out.push('\n');
    }
    out.trim().to_string()
}

/// Extract a compact summary from an assistant reply for memory retrieval.
/// Prefers the "What I'm hearing" section that Think mode produces, then falls
/// back to the first substantial paragraph.
pub fn summarize_reply(reply: &str, max: usize) -> String {
    let without_prompt = match (reply.find("<prompt>"), reply.find("</prompt>")) {
        (Some(start), Some(end)) if end > start => {
            format!("{}{}", &reply[..start], &reply[end + "</prompt>".len()..])
        }
        _ => reply.to_string(),
    };
    for line in without_prompt.lines() {
        let plain = line.replace("**", "");
        let plain = plain.trim();
        let lower = plain.to_lowercase();
        for prefix in ["what i'm hearing:", "what i’m hearing:"] {
            if lower.starts_with(prefix) {
                let text: String = plain.chars().skip(prefix.chars().count()).collect();
                if !text.trim().is_empty() {
                    return truncate_chars(text.trim(), max);
                }
            }
        }
    }
    let paragraph = without_prompt
        .split("\n\n")
        .map(strip_markdown)
        .find(|p| p.chars().count() > 40)
        .unwrap_or_else(|| strip_markdown(&without_prompt));
    truncate_chars(&collapse_whitespace(&paragraph), max)
}

/// The contents of the last `<prompt>…</prompt>` block in a reply, if any.
pub fn extract_prompt_block(reply: &str) -> Option<String> {
    let start = reply.rfind("<prompt>")?;
    let rest = &reply[start + "<prompt>".len()..];
    let end = rest.find("</prompt>")?;
    let content = rest[..end].trim();
    (!content.is_empty()).then(|| content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd_handles_epoch_and_leap_days() {
        assert_eq!(ymd_utc(0), "1970-01-01");
        assert_eq!(ymd_utc(951_782_400_000), "2000-02-29");
        assert_eq!(ymd_utc(1_790_812_800_000), "2026-10-01");
    }

    #[test]
    fn truncate_prefers_word_boundaries() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(
            truncate_chars("the quick brown fox jumps", 12),
            "the quick…"
        );
    }

    #[test]
    fn titles_come_from_the_first_sentence() {
        assert_eq!(
            derive_title("  i need to finish my physics work. Then email Dr. Lignos"),
            "I need to finish my physics work"
        );
        assert_eq!(derive_title("\n\n"), "Untitled thought");
        let long = derive_title(
            "I've been thinking about starting a philosophy club but I don't know what we'd actually do and I'm already busy",
        );
        assert!(long.chars().count() <= 57, "{long}");
        assert!(long.ends_with('…'));
    }

    #[test]
    fn summary_prefers_what_im_hearing() {
        let reply = "Got it.\n\n**What I'm hearing:** You want to start a club but worry about time.\n\n**The tension:** ...";
        assert_eq!(
            summarize_reply(reply, 200),
            "You want to start a club but worry about time."
        );
        let plain =
            "Short.\n\nThis is a longer paragraph that should become the summary of the reply.";
        assert_eq!(
            summarize_reply(plain, 200),
            "This is a longer paragraph that should become the summary of the reply."
        );
    }

    #[test]
    fn extracts_last_prompt_block() {
        let reply = "Here you go:\n<prompt>\nYou are a historian.\n</prompt>\nAssumed: essay.";
        assert_eq!(
            extract_prompt_block(reply).as_deref(),
            Some("You are a historian.")
        );
        assert_eq!(extract_prompt_block("no prompt"), None);
        assert_eq!(extract_prompt_block("<prompt>  </prompt>"), None);
    }
}
